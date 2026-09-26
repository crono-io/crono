//! Deterministic execution-input merging, narrow argument templates, and the
//! internal serializable worker execution-event vocabulary.
//!
//! Job, Target Set, Target, and invocation inputs are JSON objects merged from
//! least to most specific. Argument templates can read scalar leaves through
//! dot-separated paths, but cannot execute expressions or alter argv shape.
//! The input functions are shared by the server and browser so previews match
//! the authoritative snapshot produced before dispatch. The event module is
//! shared with the worker; it carries data but no renderer or transport.

pub mod event;

use serde_json::{Map, Value};
use std::{error::Error, fmt};

/// Maximum encoded size of inputs accepted by both the server and worker.
pub const MAX_INPUT_BYTES: usize = 65_536;

/// A deterministic validation or rendering failure safe to show beside input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    NotObject,
    TooLarge,
    InvalidKey(String),
    NulCharacter,
    InvalidTemplate(String),
    MissingVariable(String),
    NonScalarVariable(String),
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotObject => formatter.write_str("inputs must be a JSON object"),
            Self::TooLarge => write!(
                formatter,
                "inputs must not exceed {MAX_INPUT_BYTES} encoded bytes"
            ),
            Self::InvalidKey(key) => write!(
                formatter,
                "input key {key:?} must start with a lowercase letter or '_' and contain only lowercase letters, numbers, '_' and '-'"
            ),
            Self::NulCharacter => {
                formatter.write_str("input string values must not contain NUL characters")
            }
            Self::InvalidTemplate(message) => formatter.write_str(message),
            Self::MissingVariable(path) => {
                write!(formatter, "template variable {path:?} is not defined")
            }
            Self::NonScalarVariable(path) => write!(
                formatter,
                "template variable {path:?} must resolve to a string, number, or boolean"
            ),
        }
    }
}

impl Error for InputError {}

/// Validate an input object, its template-addressable keys, and encoded size.
///
/// # Errors
///
/// Rejects non-object roots, path-unsafe object keys, string values containing
/// NUL (which PostgreSQL `jsonb` cannot store and argv cannot carry), or
/// values above the worker's bounded temporary-file limit.
pub fn validate_inputs(value: &Value) -> Result<(), InputError> {
    if !value.is_object() {
        return Err(InputError::NotObject);
    }
    validate_entries(value)?;
    let encoded = serde_json::to_vec(value).map_err(|error| {
        InputError::InvalidTemplate(format!("inputs could not be encoded: {error}"))
    })?;
    if encoded.len() > MAX_INPUT_BYTES {
        return Err(InputError::TooLarge);
    }
    Ok(())
}

/// Recursively merge object layers from least to most specific.
///
/// Nested objects merge by key. Arrays, scalars, and null replace the previous
/// value as a whole. Every layer and the final result are validated.
///
/// # Errors
///
/// Returns validation failures for any input layer or the merged result.
pub fn merge_inputs(layers: &[&Value]) -> Result<Value, InputError> {
    let mut merged = Value::Object(Map::new());
    for layer in layers {
        validate_inputs(layer)?;
        merge_value(&mut merged, layer);
    }
    validate_inputs(&merged)?;
    Ok(merged)
}

/// Validate all templates without requiring their variables to exist yet.
///
/// # Errors
///
/// Rejects unclosed, empty, or path-invalid placeholders.
pub fn validate_argument_templates(arguments: &[String]) -> Result<(), InputError> {
    for argument in arguments {
        parse_template(argument, None)?;
    }
    Ok(())
}

/// Render argument templates against a merged input object.
///
/// Each template remains one argv item. Scalar values are inserted verbatim;
/// missing or structured values fail before the process can be dispatched.
///
/// # Errors
///
/// Returns template syntax, missing-variable, or non-scalar failures.
pub fn render_arguments(arguments: &[String], inputs: &Value) -> Result<Vec<String>, InputError> {
    validate_inputs(inputs)?;
    arguments
        .iter()
        .map(|argument| parse_template(argument, Some(inputs)))
        .collect()
}

/// Return renderable leaf paths for preview and autocomplete displays.
#[must_use]
pub fn scalar_paths(inputs: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_scalar_paths(inputs, "", &mut paths);
    paths
}

fn validate_entries(value: &Value) -> Result<(), InputError> {
    match value {
        Value::Object(values) => {
            for (key, nested) in values {
                if !valid_segment(key) {
                    return Err(InputError::InvalidKey(key.clone()));
                }
                validate_entries(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                validate_entries(nested)?;
            }
        }
        Value::String(text) if text.contains('\0') => return Err(InputError::NulCharacter),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn valid_segment(segment: &str) -> bool {
    let mut characters = segment.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first == '_') {
        return false;
    }
    characters.all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-')
    })
}

fn merge_value(base: &mut Value, overlay: &Value) {
    if let (Some(base_values), Some(overlay_values)) = (base.as_object_mut(), overlay.as_object()) {
        for (key, value) in overlay_values {
            if let Some(existing) = base_values.get_mut(key) {
                merge_value(existing, value);
            } else {
                base_values.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overlay.clone();
    }
}

fn parse_template(template: &str, inputs: Option<&Value>) -> Result<String, InputError> {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(open) = remaining.find("{{") {
        let prefix = remaining.get(..open).ok_or_else(|| {
            InputError::InvalidTemplate("template boundary is invalid".to_string())
        })?;
        if let Some(literal_prefix) = prefix.strip_suffix('\\') {
            output.push_str(literal_prefix);
            output.push_str("{{");
            remaining = remaining.get(open + 2..).ok_or_else(|| {
                InputError::InvalidTemplate("template boundary is invalid".to_string())
            })?;
            continue;
        }
        output.push_str(prefix);
        let after_open = remaining.get(open + 2..).ok_or_else(|| {
            InputError::InvalidTemplate("template boundary is invalid".to_string())
        })?;
        let close = after_open.find("}}").ok_or_else(|| {
            InputError::InvalidTemplate("template placeholder is not closed".to_string())
        })?;
        let path = after_open
            .get(..close)
            .ok_or_else(|| InputError::InvalidTemplate("template boundary is invalid".to_string()))?
            .trim();
        validate_path(path)?;
        if let Some(values) = inputs {
            output.push_str(&render_path(values, path)?);
        } else {
            output.push_str("{{");
            output.push_str(path);
            output.push_str("}}");
        }
        remaining = after_open.get(close + 2..).ok_or_else(|| {
            InputError::InvalidTemplate("template boundary is invalid".to_string())
        })?;
    }
    output.push_str(remaining);
    Ok(output)
}

fn validate_path(path: &str) -> Result<(), InputError> {
    if path.is_empty() || !path.split('.').all(valid_segment) {
        return Err(InputError::InvalidTemplate(format!(
            "template path {path:?} must contain dot-separated input keys"
        )));
    }
    Ok(())
}

fn render_path(inputs: &Value, path: &str) -> Result<String, InputError> {
    let mut value = inputs;
    for segment in path.split('.') {
        value = value
            .as_object()
            .and_then(|object| object.get(segment))
            .ok_or_else(|| InputError::MissingVariable(path.to_string()))?;
    }
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err(InputError::NonScalarVariable(path.to_string()))
        }
    }
}

fn collect_scalar_paths(value: &Value, prefix: &str, paths: &mut Vec<String>) {
    if let Value::Object(values) = value {
        for (key, nested) in values {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if matches!(nested, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
                paths.push(path);
            } else {
                collect_scalar_paths(nested, &path, paths);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InputError, merge_inputs, render_arguments, scalar_paths, validate_inputs};
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn inputs_reject_nul_in_string_values() {
        assert_eq!(
            validate_inputs(&json!({"name": "a\u{0}b"})),
            Err(InputError::NulCharacter)
        );
        assert_eq!(
            validate_inputs(&json!({"nested": {"list": ["ok", "\u{0}"]}})),
            Err(InputError::NulCharacter)
        );
        assert_eq!(validate_inputs(&json!({"name": "plain"})), Ok(()));
    }

    #[test]
    fn merges_objects_recursively_and_replaces_other_values() -> Result<()> {
        let job = json!({"network": {"timeout": 10, "headers": ["a"]}, "region": "eu"});
        let target_set = json!({"network": {"timeout": 20}});
        let target = json!({"network": {"headers": ["b"]}});
        let invocation = json!({"region": null});
        let merged = merge_inputs(&[&job, &target_set, &target, &invocation])?;
        assert_eq!(
            merged,
            json!({"network": {"timeout": 20, "headers": ["b"]}, "region": null})
        );
        Ok(())
    }

    #[test]
    fn renders_scalar_paths_without_changing_argument_boundaries() -> Result<()> {
        let inputs = json!({"endpoint": {"url": "https://example.test/a b"}, "attempts": 3});
        let rendered = render_arguments(
            &[
                "--url={{ endpoint.url }}".to_string(),
                "{{attempts}}".to_string(),
            ],
            &inputs,
        )?;
        assert_eq!(rendered, vec!["--url=https://example.test/a b", "3"]);
        assert_eq!(scalar_paths(&inputs), vec!["attempts", "endpoint.url"]);
        Ok(())
    }

    #[test]
    fn supports_literal_opening_delimiters() -> Result<()> {
        let rendered = render_arguments(&[r"value=\{{literal}}".to_string()], &json!({}))?;
        assert_eq!(rendered, vec!["value={{literal}}"]);
        Ok(())
    }

    #[test]
    fn rejects_invalid_inputs_and_unrenderable_values() {
        assert_eq!(validate_inputs(&json!([])), Err(InputError::NotObject));
        assert!(matches!(
            validate_inputs(&json!({"Bad": 1})),
            Err(InputError::InvalidKey(_))
        ));
        assert!(matches!(
            render_arguments(&["{{missing}}".to_string()], &json!({})),
            Err(InputError::MissingVariable(_))
        ));
        assert!(matches!(
            render_arguments(&["{{nested}}".to_string()], &json!({"nested": {}})),
            Err(InputError::NonScalarVariable(_))
        ));
    }
}
