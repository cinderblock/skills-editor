use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::paths::{claude_dir, home_dir, strip_verbatim};
use crate::settings::Settings;

#[derive(Serialize, Clone)]
pub struct Skill {
    /// Stable id: the skill directory's absolute path.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Absolute path of the skill directory.
    pub dir: String,
    /// Absolute path of the SKILL.md file.
    pub skill_md: String,
    /// All files inside the skill dir, as sorted forward-slash relative paths.
    pub files: Vec<String>,
    /// True when the skill is exactly one SKILL.md and nothing else.
    pub single_file: bool,
    pub editable: bool,
    /// True when a skillOverrides entry or a disabled plugin turns it off.
    pub disabled: bool,
}

#[derive(Serialize, Clone)]
pub struct SkillGroup {
    pub key: String,
    /// "user" | "project" | "plugin" | "extra"
    pub kind: String,
    /// Logical name shown in the tree (e.g. project folder name).
    pub label: String,
    /// The concrete filesystem location backing the group.
    pub detail: String,
    pub skills: Vec<Skill>,
}

const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "__pycache__", ".venv"];

fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Extract `name` / `description` from a SKILL.md frontmatter block.
fn parse_frontmatter(skill_md: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = fs::read_to_string(skill_md) else {
        return (None, None);
    };
    let Some(yaml) = extract_frontmatter(&text) else {
        return (None, None);
    };
    match serde_yaml::from_str::<serde_yaml::Value>(&yaml) {
        Ok(v) => {
            let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
            (get("name"), get("description"))
        }
        Err(_) => (None, None),
    }
}

/// The raw YAML between leading `---` fences, if present.
pub fn extract_frontmatter(text: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    for fence in ["\n---\r\n", "\n---\n", "\r\n---\r\n", "\r\n---\n"] {
        if let Some(end) = rest.find(fence) {
            return Some(rest[..end].to_string());
        }
    }
    // Frontmatter that ends the file.
    let trimmed = rest.trim_end();
    trimmed
        .strip_suffix("\n---")
        .or_else(|| trimmed.strip_suffix("\r\n---"))
        .map(|s| s.to_string())
}

/// skillOverrides from one settings file: skill name -> "off"/"on".
fn read_overrides_file(file: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(file) else {
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    v.get("skillOverrides")
        .and_then(|o| o.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Merged skillOverrides for a `.claude`-style dir (settings.local.json wins).
fn skill_overrides(claude_like_dir: &Path) -> HashMap<String, String> {
    let mut merged = read_overrides_file(&claude_like_dir.join("settings.json"));
    merged.extend(read_overrides_file(&claude_like_dir.join("settings.local.json")));
    merged
}

/// Plugins turned off via enabledPlugins in user settings ("plugin@marketplace").
fn disabled_plugins() -> HashSet<String> {
    let Ok(claude) = claude_dir() else { return HashSet::new() };
    let mut set = HashSet::new();
    for name in ["settings.json", "settings.local.json"] {
        let Ok(text) = fs::read_to_string(claude.join(name)) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if let Some(obj) = v.get("enabledPlugins").and_then(|o| o.as_object()) {
            for (key, val) in obj {
                if val.as_bool() == Some(false) {
                    set.insert(key.clone());
                } else {
                    set.remove(key);
                }
            }
        }
    }
    set
}

fn is_off(overrides: &HashMap<String, String>, dir_name: &str) -> bool {
    overrides.get(dir_name).map(|v| v == "off").unwrap_or(false)
}

/// Build a Skill from a directory that contains SKILL.md.
fn load_skill(dir: &Path, editable: bool, disabled: bool) -> Option<Skill> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.is_file() {
        return None;
    }
    let mut files: Vec<String> = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !e.file_name().to_str().map(skip_dir).unwrap_or(false)
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(rel) = entry.path().strip_prefix(dir) {
                files.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files.sort();
    let single_file = files.len() == 1 && files[0] == "SKILL.md";
    let (fm_name, fm_desc) = parse_frontmatter(&skill_md);
    let dir = strip_verbatim(dir);
    Some(Skill {
        id: dir.to_string_lossy().to_string(),
        name: fm_name.unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".into())
        }),
        description: fm_desc.unwrap_or_default(),
        dir: dir.to_string_lossy().to_string(),
        skill_md: dir.join("SKILL.md").to_string_lossy().to_string(),
        files,
        single_file,
        editable,
        disabled,
    })
}

/// Scan a "skills root" — a directory whose children are skill directories.
/// `overrides` is the merged skillOverrides map governing this root.
pub fn scan_skills_root(
    root: &Path,
    editable: bool,
    overrides: &HashMap<String, String>,
) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let disabled = is_off(overrides, &e.file_name().to_string_lossy());
            load_skill(&e.path(), editable, disabled)
        })
        .collect();
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

/// Project paths registered in ~/.claude.json (`projects` object keys).
fn claude_project_paths() -> Vec<PathBuf> {
    let Ok(home) = home_dir() else { return Vec::new() };
    let file = home.join(".claude.json");
    let Ok(text) = fs::read_to_string(&file) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v.get("projects")
        .and_then(|p| p.as_object())
        .map(|o| o.keys().map(PathBuf::from).collect())
        .unwrap_or_default()
}

/// "plugin@marketplace" derived from a path under plugins/cache, if possible.
fn plugin_key(plugin_dir: &Path) -> Option<String> {
    let comps: Vec<String> = plugin_dir
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let cache_idx = comps.iter().position(|c| c == "cache")?;
    let marketplace = comps.get(cache_idx + 1)?;
    let plugin = comps.get(cache_idx + 2)?;
    Some(format!("{plugin}@{marketplace}"))
}

/// Plugin-provided skills under ~/.claude/plugins (marketplace clones).
fn plugin_groups(user_overrides: &HashMap<String, String>) -> Vec<SkillGroup> {
    let Ok(claude) = claude_dir() else { return Vec::new() };
    let plugins = claude.join("plugins");
    if !plugins.is_dir() {
        return Vec::new();
    }
    let off_plugins = disabled_plugins();
    // plugin dir -> skills found; keyed by the directory that contains "skills/"
    let mut by_plugin: BTreeMap<PathBuf, Vec<Skill>> = BTreeMap::new();
    for entry in WalkDir::new(&plugins)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_entry(|e| {
            !e.file_type().is_dir()
                || !e.file_name().to_str().map(skip_dir).unwrap_or(false)
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "SKILL.md" {
            let skill_dir = entry.path().parent();
            let skills_parent = skill_dir.and_then(|d| d.parent());
            if let (Some(skill_dir), Some(skills_parent)) = (skill_dir, skills_parent) {
                if skills_parent.file_name().and_then(|n| n.to_str()) == Some("skills") {
                    if let Some(plugin_dir) = skills_parent.parent() {
                        let skill_dir_name = skill_dir
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let disabled = plugin_key(plugin_dir)
                            .map(|k| off_plugins.contains(&k))
                            .unwrap_or(false)
                            || is_off(user_overrides, &skill_dir_name);
                        if let Some(skill) = load_skill(skill_dir, false, disabled) {
                            by_plugin
                                .entry(plugin_dir.to_path_buf())
                                .or_default()
                                .push(skill);
                        }
                    }
                }
            }
        }
    }
    by_plugin
        .into_iter()
        .map(|(plugin_dir, mut skills)| {
            skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            let plugin_dir = strip_verbatim(&plugin_dir);
            let label = plugin_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "plugin".into());
            SkillGroup {
                key: format!("plugin:{}", plugin_dir.display()),
                kind: "plugin".into(),
                label,
                detail: plugin_dir.to_string_lossy().to_string(),
                skills,
            }
        })
        .collect()
}

pub fn discover(settings: &Settings) -> Result<Vec<SkillGroup>, String> {
    let mut groups: Vec<SkillGroup> = Vec::new();

    // User-level skills.
    let claude = claude_dir()?;
    let user_overrides = skill_overrides(&claude);
    let user_root = claude.join("skills");
    groups.push(SkillGroup {
        key: "user".into(),
        kind: "user".into(),
        label: "User skills".into(),
        detail: user_root.to_string_lossy().to_string(),
        skills: scan_skills_root(&user_root, true, &user_overrides),
    });

    // Project-level skills, from ~/.claude.json's real project paths.
    let mut project_groups: Vec<SkillGroup> = Vec::new();
    for project in claude_project_paths() {
        let project_claude = project.join(".claude");
        let root = project_claude.join("skills");
        if !root.is_dir() {
            continue;
        }
        // Project skills honor the project's overrides, falling back to user ones.
        let mut overrides = user_overrides.clone();
        overrides.extend(skill_overrides(&project_claude));
        let skills = scan_skills_root(&root, true, &overrides);
        if skills.is_empty() {
            continue;
        }
        let label = project
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| project.to_string_lossy().to_string());
        project_groups.push(SkillGroup {
            key: format!("project:{}", project.display()),
            kind: "project".into(),
            label,
            detail: root.to_string_lossy().to_string(),
            skills,
        });
    }
    project_groups.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    groups.extend(project_groups);

    // Extra roots from settings — treated like additional skills roots.
    for root in &settings.extra_roots {
        let root_path = PathBuf::from(root);
        // If the root sits inside a `.claude`-style dir, honor its overrides too.
        let mut overrides = user_overrides.clone();
        if let Some(parent) = root_path.parent() {
            overrides.extend(skill_overrides(parent));
        }
        let skills = scan_skills_root(&root_path, true, &overrides);
        groups.push(SkillGroup {
            key: format!("extra:{root}"),
            kind: "extra".into(),
            label: root_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| root.clone()),
            detail: root.clone(),
            skills,
        });
    }

    // Plugin cache skills (read-only).
    groups.extend(plugin_groups(&user_overrides));

    Ok(groups)
}

/// Roots that file read/write operations are allowed to touch.
pub fn allowed_roots(settings: &Settings) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(claude) = claude_dir() {
        roots.push(claude.join("skills"));
        roots.push(claude.join("plugins"));
    }
    for project in claude_project_paths() {
        roots.push(project.join(".claude").join("skills"));
    }
    for root in &settings.extra_roots {
        roots.push(PathBuf::from(root));
    }
    roots
}
