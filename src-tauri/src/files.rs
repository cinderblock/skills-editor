use std::fs;
use std::path::{Path, PathBuf};

use crate::discovery::allowed_roots;
use crate::paths::is_within;
use crate::settings::Settings;

/// Guard: the path must live inside a known skills root, a hooks/agents dir,
/// or be a script a discovered hook runs.
fn check_allowed(path: &Path, settings: &Settings) -> Result<(), String> {
    let within = |roots: Vec<PathBuf>| roots.iter().any(|root| is_within(path, root));
    // Skills roots are cheap to compute; hook discovery only when needed.
    if within(allowed_roots(settings)) || within(crate::hooks::allowed_paths(settings)) {
        Ok(())
    } else {
        Err(format!(
            "{} is outside every known skills or hooks location; refusing to touch it",
            path.display()
        ))
    }
}

pub fn read_text(path: &str, settings: &Settings) -> Result<String, String> {
    let path = PathBuf::from(path);
    check_allowed(&path, settings)?;
    fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

pub fn write_text(path: &str, content: &str, settings: &Settings) -> Result<(), String> {
    let path = PathBuf::from(path);
    // For new files the file itself doesn't exist yet — validate the parent.
    let check_target = if path.exists() {
        path.clone()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "path has no parent".to_string())?
    };
    check_allowed(&check_target, settings)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create dir: {e}"))?;
    }
    fs::write(&path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn create_skill(
    root: &str,
    name: &str,
    description: &str,
    settings: &Settings,
) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("skill name is empty".into());
    }
    if name.contains(['/', '\\', ':']) {
        return Err("skill name must be a plain directory name".into());
    }
    let root = PathBuf::from(root);
    check_allowed(&root, settings)?;
    let dir = root.join(name.trim());
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create skill dir: {e}"))?;
    let skill_md = dir.join("SKILL.md");
    let body = format!(
        "---\nname: {}\ndescription: {}\n---\n\n# {}\n\n",
        name.trim(),
        description.trim(),
        name.trim()
    );
    fs::write(&skill_md, body).map_err(|e| format!("cannot write SKILL.md: {e}"))?;
    Ok(skill_md.to_string_lossy().to_string())
}

pub fn delete_skill(dir: &str, settings: &Settings) -> Result<(), String> {
    let dir = PathBuf::from(dir);
    check_allowed(&dir, settings)?;
    // Only delete things that actually look like a skill.
    if !dir.join("SKILL.md").is_file() {
        return Err(format!(
            "{} does not contain a SKILL.md; refusing to delete",
            dir.display()
        ));
    }
    remove_dir_all_robust(&dir)
}

/// remove_dir_all that clears read-only attributes first (git objects on
/// Windows are read-only and make the plain call fail).
pub fn remove_dir_all_robust(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mut perms = md.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(entry.path(), perms);
        }
    }
    fs::remove_dir_all(dir).map_err(|e| format!("cannot remove {}: {e}", dir.display()))
}
