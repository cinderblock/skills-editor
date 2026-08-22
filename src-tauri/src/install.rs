use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::files::remove_dir_all_robust;
use crate::git;
use crate::paths::claude_dir;

/// Install a skill from a git repo: shallow-clone to a temp dir and copy the
/// skill's subdirectory into the user skills root.
pub fn install_skill(
    repo_url: &str,
    subpath: &str,
    name: &str,
    overwrite: bool,
) -> Result<String, String> {
    if name.trim().is_empty() || name.contains(['/', '\\', ':']) {
        return Err("invalid skill name".into());
    }
    let dest_root = claude_dir()?.join("skills");
    fs::create_dir_all(&dest_root).map_err(|e| format!("cannot create skills dir: {e}"))?;
    let dest = dest_root.join(name.trim());
    if dest.exists() && !overwrite {
        return Err(format!(
            "{} already exists — enable overwrite to replace it",
            dest.display()
        ));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("skills-editor-install-{nonce}"));
    fs::create_dir_all(&tmp).map_err(|e| format!("cannot create temp dir: {e}"))?;

    let result = (|| -> Result<String, String> {
        git::run_in(&tmp, &["clone", "--depth", "1", repo_url, "clone"])?;
        let src = {
            let base = tmp.join("clone");
            let sub = subpath.trim_matches('/');
            if sub.is_empty() { base } else { base.join(sub.replace('/', std::path::MAIN_SEPARATOR_STR)) }
        };
        if !src.join("SKILL.md").is_file() {
            return Err(format!(
                "{subpath} in {repo_url} does not contain a SKILL.md — not a skill"
            ));
        }
        if dest.exists() {
            remove_dir_all_robust(&dest)?;
        }
        copy_skill_dir(&src, &dest)?;
        Ok(format!("Installed {} to {}", name.trim(), dest.display()))
    })();

    // Best-effort temp cleanup either way.
    let _ = remove_dir_all_robust(&tmp);
    result
}

fn copy_skill_dir(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_skill_dir(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("cannot copy {}: {e}", src_path.display()))?;
        }
    }
    Ok(())
}
