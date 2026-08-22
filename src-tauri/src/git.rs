use std::path::Path;
use std::process::Command;

use crate::paths::strip_verbatim;

/// Run git with fixed args against a repo directory, capturing output.
///
/// All callers pass literal argument lists — nothing here ever builds a
/// force-push (`--force`, `-f`, `+refspec`), and no argument is derived from
/// remote input. Keep it that way.
pub fn run(repo: &Path, args: &[&str]) -> Result<String, String> {
    let repo = strip_verbatim(repo);
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(args)
        .output()
        .map_err(|e| format!("failed to launch git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let mut msg = format!("git {} failed", args.join(" "));
        let detail = if stderr.trim().is_empty() { &stdout } else { &stderr };
        if !detail.trim().is_empty() {
            msg.push_str(&format!(": {}", detail.trim()));
        }
        Err(msg)
    }
}

/// Like `run`, but from an arbitrary working directory (used for `git clone`).
pub fn run_in(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let cwd = strip_verbatim(cwd);
    let output = Command::new("git")
        .current_dir(&cwd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to launch git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let mut msg = format!("git {} failed", args.join(" "));
        let detail = if stderr.trim().is_empty() { &stdout } else { &stderr };
        if !detail.trim().is_empty() {
            msg.push_str(&format!(": {}", detail.trim()));
        }
        Err(msg)
    }
}

pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}
