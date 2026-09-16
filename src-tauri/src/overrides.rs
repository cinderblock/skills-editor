use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The `.claude`-style directory whose settings files govern a skill.
///
/// Mirrors discovery: a skill lives at `<root>/<name>` and its overrides are
/// read from `<root>`'s parent (`~/.claude`, `<project>/.claude`, or the
/// parent of an extra root).
fn scope_dir(skill_dir: &Path) -> Result<PathBuf, String> {
    let lower = skill_dir.to_string_lossy().to_lowercase().replace('/', "\\");
    if lower.contains("\\plugins\\cache\\") || lower.contains("\\plugins\\data\\") {
        return Err(
            "plugin skills can't be toggled individually — disable the plugin in Claude Code instead"
                .into(),
        );
    }
    skill_dir
        .parent()
        .and_then(|root| root.parent())
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "cannot resolve settings scope for this skill".to_string())
}

/// Apply an edit to the skillOverrides object of one settings file.
/// `create` controls whether a missing file/object is created.
fn edit_file(
    file: &Path,
    create: bool,
    apply: impl FnOnce(&mut Map<String, Value>),
) -> Result<(), String> {
    let mut root: Value = match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON: {e}", file.display()))?,
        Err(_) if create => Value::Object(Map::new()),
        Err(_) => return Ok(()),
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", file.display()))?;
    if !obj.contains_key("skillOverrides") {
        if !create {
            return Ok(());
        }
        obj.insert("skillOverrides".into(), Value::Object(Map::new()));
    }
    let overrides = obj
        .get_mut("skillOverrides")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| format!("skillOverrides in {} is not an object", file.display()))?;
    apply(overrides);
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(file, text + "\n").map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// Enable or disable a skill by editing the governing settings files.
///
/// Disable: sets `"<name>": "off"` in settings.json. Enable: removes the
/// entry from BOTH settings.json and settings.local.json, so a stale local
/// override can't keep the skill off.
pub fn set_skill_enabled(skill_dir: &str, enabled: bool) -> Result<String, String> {
    let dir = PathBuf::from(skill_dir);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "skill dir has no name".to_string())?;
    let scope = scope_dir(&dir)?;
    let main = scope.join("settings.json");
    let local = scope.join("settings.local.json");

    // Clear the key everywhere first; then write "off" if disabling.
    edit_file(&local, false, |o| {
        o.shift_remove(&name);
    })?;
    if enabled {
        edit_file(&main, false, |o| {
            o.shift_remove(&name);
        })?;
        Ok(format!("Enabled {name}"))
    } else {
        edit_file(&main, true, |o| {
            o.insert(name.clone(), Value::String("off".into()));
        })?;
        Ok(format!("Disabled {name} (skillOverrides in {})", main.display()))
    }
}
