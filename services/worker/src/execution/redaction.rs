//! Conservative sanitization of observable inputs, commands, and process output.
//!
//! Key names are a fallback because Jobs currently lack explicit secret types.
//! Known sensitive input and credential-switch values are replaced even when
//! echoed by a child. Unknown literal credentials and transformed values cannot be
//! recognized; dedicated secret metadata is needed before persistence.

use serde_json::Value;

/// Redacts input leaves identified by sensitive keys, including occurrences in
/// command arguments and stdout/stderr. The raw values remain only in memory.
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    #[must_use]
    pub fn from_inputs(inputs: &Value) -> Self {
        Self::from_inputs_and_arguments(inputs, &[])
    }

    /// Include values of recognizable credential switches in the redaction set
    /// so a child echoing its literal argv cannot expose them in output events.
    #[must_use]
    pub fn from_inputs_and_arguments(inputs: &Value, arguments: &[String]) -> Self {
        let mut secrets = Vec::new();
        collect_secrets(inputs, false, &mut secrets);
        collect_argument_secrets(arguments, &mut secrets);
        secrets.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        secrets.dedup();
        Self { secrets }
    }

    #[must_use]
    pub fn text(&self, value: &str) -> String {
        self.secrets.iter().fold(value.to_string(), |safe, secret| {
            safe.replace(secret, "<redacted>")
        })
    }

    #[must_use]
    pub fn values(&self, inputs: &Value) -> Value {
        match inputs {
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            if sensitive_key(key) {
                                Value::String("<redacted>".to_string())
                            } else {
                                self.values(value)
                            },
                        )
                    })
                    .collect(),
            ),
            Value::Array(values) => {
                Value::Array(values.iter().map(|value| self.values(value)).collect())
            }
            Value::String(value) => Value::String(self.text(value)),
            value => value.clone(),
        }
    }

    /// Sanitize argv while hiding values of recognizable credential switches.
    #[must_use]
    pub fn arguments(&self, arguments: &[String]) -> Vec<String> {
        let mut hide_next = false;
        arguments
            .iter()
            .map(|argument| {
                if hide_next {
                    hide_next = false;
                    return "<redacted>".to_string();
                }
                let safe = self.text(argument);
                if let Some((key, _)) = safe.split_once('=')
                    && sensitive_key(key)
                {
                    return format!("{key}=<redacted>");
                }
                if let Some((key, _)) = safe.split_once(':')
                    && sensitive_key(key)
                {
                    return format!("{key}: <redacted>");
                }
                if sensitive_switch(&safe) {
                    hide_next = true;
                } else if sensitive_key(&safe) {
                    return "<redacted>".to_string();
                }
                safe
            })
            .collect()
    }
}

fn sensitive_switch(value: &str) -> bool {
    value.starts_with('-') && !value.contains(['=', ':', '/', '?', ' ']) && sensitive_key(value)
}

fn collect_argument_secrets(arguments: &[String], secrets: &mut Vec<String>) {
    let mut next_is_secret = false;
    for argument in arguments {
        if next_is_secret {
            collect_secret_text(argument, secrets);
            next_is_secret = false;
            continue;
        }
        next_is_secret = sensitive_switch(argument);
        for part in argument.split(['?', '&']) {
            if let Some((key, value)) = part.split_once('=')
                && sensitive_key(key)
            {
                collect_secret_text(value, secrets);
            }
        }
        if let Some((key, value)) = argument.split_once(':')
            && sensitive_key(key)
        {
            collect_secret_text(value.trim(), secrets);
            if let Some(last) = value.split_whitespace().last() {
                collect_secret_text(last, secrets);
            }
        }
    }
}

fn collect_secret_text(value: &str, secrets: &mut Vec<String>) {
    if !value.is_empty() {
        secrets.push(value.to_string());
        for line in value.lines().filter(|line| !line.is_empty()) {
            secrets.push(line.to_string());
        }
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "authorization",
        "credential",
        "privatekey",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn collect_secrets(value: &Value, inherited: bool, secrets: &mut Vec<String>) {
    match value {
        Value::Object(values) => {
            for (key, child) in values {
                collect_secrets(child, inherited || sensitive_key(key), secrets);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_secrets(child, inherited, secrets);
            }
        }
        Value::String(value) if inherited => collect_secret_text(value, secrets),
        Value::Number(value) if inherited => secrets.push(value.to_string()),
        Value::Bool(value) if inherited => secrets.push(value.to_string()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::Redactor;
    use serde_json::json;

    #[test]
    fn hides_sensitive_inputs_and_substituted_arguments() {
        let inputs = json!({"name":"world", "password":"foo", "nested":{"VAULT_TOKEN":"bar"}});
        let redactor = Redactor::from_inputs(&inputs);
        assert_eq!(
            redactor.values(&inputs),
            json!({"name":"world", "password":"<redacted>", "nested":{"VAULT_TOKEN":"<redacted>"}})
        );
        let arguments = redactor.arguments(&[
            "hello world".to_string(),
            "Bearer bar".to_string(),
            "--password=foo".to_string(),
            "Authorization: Bearer literal".to_string(),
            "--token".to_string(),
            "literal".to_string(),
            "--url=https://example.test/?token=literal".to_string(),
        ]);
        assert_eq!(
            arguments,
            [
                "hello world",
                "Bearer <redacted>",
                "--password=<redacted>",
                "Authorization: <redacted>",
                "--token",
                "<redacted>",
                "<redacted>"
            ]
        );
        assert_eq!(redactor.text("foo/bar"), "<redacted>/<redacted>");
    }

    #[test]
    fn multiline_secret_is_hidden_when_child_prints_separate_lines() {
        let redactor = Redactor::from_inputs(&json!({"ssh_private_key":"first-line\nsecond-line"}));
        assert_eq!(redactor.text("first-line"), "<redacted>");
        assert_eq!(redactor.text("second-line"), "<redacted>");
    }

    #[test]
    fn literal_credential_arguments_are_hidden_if_echoed() {
        let args = [
            "--token".to_string(),
            "literal".to_string(),
            "--url=https://example.test/?api_key=querysecret".to_string(),
        ];
        let redactor = Redactor::from_inputs_and_arguments(&json!({}), &args);
        assert_eq!(
            redactor.text("literal querysecret"),
            "<redacted> <redacted>"
        );
        assert_eq!(
            redactor.arguments(&args),
            ["--token", "<redacted>", "<redacted>"]
        );
    }
}
