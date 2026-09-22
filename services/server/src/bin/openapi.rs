//! Emit the generated Crono server `OpenAPI` document.

use anyhow::Result;

fn main() -> Result<()> {
    let document = crono_server::api::openapi();
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}
