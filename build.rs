//! Capture native service metadata and refresh it when Git references change.

use std::{env, error::Error, path::PathBuf, process::Command};

/// Generate metadata for the calling package, including source archive builds.
fn main() -> Result<(), Box<dyn Error>> {
    built::write_built_file()?;

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    println!("cargo:rerun-if-changed={}", manifest.display());

    // Git resolves linked worktrees and packed references correctly. Detection
    // still works through built's git2 backend if the Git executable is absent.
    for name in ["HEAD", "refs", "packed-refs"] {
        if let Ok(output) = Command::new("git")
            .current_dir(&manifest)
            .args(["rev-parse", "--git-path", name])
            .output()
            && output.status.success()
        {
            let path = manifest.join(String::from_utf8(output.stdout)?.trim());
            // Watching a nonexistent optional path would rebuild on every run.
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    Ok(())
}
