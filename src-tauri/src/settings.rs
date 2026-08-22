use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::paths::home_dir;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Settings {
    /// Path of the machine's skills tracking repo.
    pub repo_path: Option<String>,
    /// Remote URL used to share the tracking repo between hosts.
    pub remote_url: Option<String>,
    /// Additional directories to scan as skills roots.
    pub extra_roots: Vec<String>,
}

impl Settings {
    pub fn effective_repo_path(&self) -> Result<PathBuf, String> {
        match &self.repo_path {
            Some(p) if !p.trim().is_empty() => Ok(PathBuf::from(p)),
            _ => Ok(home_dir()?.join(".claude-skills-repo")),
        }
    }
}

fn settings_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no app config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let Ok(file) = settings_file(app) else {
        return Settings::default();
    };
    fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    let file = settings_file(app)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create config dir: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&file, text).map_err(|e| format!("cannot write settings: {e}"))
}
