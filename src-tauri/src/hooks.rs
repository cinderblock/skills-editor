//! Claude Code hooks: discovery, script resolution, safe edits, a disabled-hook
//! store, and a local test runner. See plans/hooks.md for the reference notes
//! this is built from.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use walkdir::WalkDir;

use crate::discovery::{self, claude_project_paths, disabled_plugins, extract_frontmatter};
use crate::paths::{claude_dir, hide_console, home_dir, path_key, strip_verbatim};
use crate::settings::Settings;

/// Every hook event Claude Code recognises. An unknown name in a settings
/// file is skipped with a warning, so the editor refuses to write one.
pub const EVENTS: [&str; 33] = [
    "SessionStart",
    "SessionEnd",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "Stop",
    "StopFailure",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Elicitation",
    "ElicitationResult",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "Notification",
    "ConfigChange",
    "InstructionsLoaded",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "MessageDisplay",
    "TeammateIdle",
];

pub const HANDLER_TYPES: [&str; 5] = ["command", "http", "mcp_tool", "prompt", "agent"];

/// File extensions treated as editable hook scripts.
const SCRIPT_EXTS: [&str; 16] = [
    "sh", "bash", "zsh", "ps1", "psm1", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "cmd",
    "bat", "lua", "fish",
];

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Content hashing (optimistic concurrency for settings files)
// ---------------------------------------------------------------------------

/// FNV-1a 64 — stable across runs, plenty to detect "changed on disk".
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

const MISSING_HASH: &str = "missing";

fn file_hash(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => content_hash(&bytes),
        Err(_) => MISSING_HASH.into(),
    }
}

// ---------------------------------------------------------------------------
// Data model sent to the UI
// ---------------------------------------------------------------------------

/// A script referenced by one handler.
#[derive(Serialize, Clone)]
pub struct ScriptRef {
    pub event: String,
    pub group: usize,
    pub handler: usize,
    pub path: String,
    pub exists: bool,
}

/// A hook the user disabled; kept by the app, not in any Claude file.
#[derive(Serialize, Deserialize, Clone)]
pub struct ParkedHook {
    pub id: String,
    /// Settings file the hook was removed from (and returns to).
    pub file: String,
    pub event: String,
    /// The group's matcher (absent = match all).
    #[serde(default)]
    pub matcher: Option<Value>,
    /// Other keys the group carried, restored with it.
    #[serde(default)]
    pub group_extra: Map<String, Value>,
    pub handler: Value,
    pub disabled_at: u64,
}

#[derive(Serialize, Clone)]
pub struct HookSource {
    /// Absolute path of the file declaring the hooks.
    pub file: String,
    /// Short name shown in the UI ("settings.json", "hooks.json", "SKILL.md").
    pub file_label: String,
    /// "user" | "project" | "local" | "managed" | "plugin" | "skill" | "agent"
    pub kind: String,
    pub editable: bool,
    pub exists: bool,
    /// Content hash at read time; writes must present it back.
    pub hash: String,
    /// The `hooks` object as found (event -> matcher groups).
    pub hooks: Value,
    pub disable_all_hooks: bool,
    /// Why these hooks don't run even though they're configured.
    pub inactive_reason: Option<String>,
    /// Value of CLAUDE_PROJECT_DIR when these hooks run, if knowable.
    pub project_dir: Option<String>,
    pub plugin_root: Option<String>,
    pub scripts: Vec<ScriptRef>,
    pub parked: Vec<ParkedHook>,
    pub parse_error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ScriptFile {
    pub path: String,
    pub name: String,
    /// False for files sitting in a hooks dir that nothing references.
    pub referenced: bool,
    pub editable: bool,
}

#[derive(Serialize, Clone)]
pub struct HookGroup {
    pub key: String,
    /// "user" | "project" | "managed" | "plugin" | "frontmatter" | "orphaned"
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub sources: Vec<HookSource>,
    pub scripts: Vec<ScriptFile>,
}

/// A settings file a new hook can be added to.
#[derive(Serialize, Clone)]
pub struct HookTarget {
    pub file: String,
    pub label: String,
    pub project_dir: Option<String>,
}

#[derive(Serialize)]
pub struct HooksOverview {
    pub groups: Vec<HookGroup>,
    pub targets: Vec<HookTarget>,
    pub events: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// Script path extraction
// ---------------------------------------------------------------------------

pub struct ScriptCtx<'a> {
    pub home: &'a Path,
    pub project_dir: Option<&'a Path>,
    pub plugin_root: Option<&'a Path>,
    /// Base for relative paths (skill dir, project dir); None = ignore them.
    pub base_dir: Option<&'a Path>,
}

/// Split a shell command into words, honoring quotes and treating shell
/// operators as separators. Good enough to spot file paths.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => quote = Some(c),
                c if c.is_whitespace() || ";&|()<>`".contains(c) => {
                    if !cur.is_empty() {
                        words.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn fwd(p: &Path) -> String {
    strip_verbatim(p).to_string_lossy().replace('\\', "/")
}

/// Turn a Git Bash style `/c/Users/...` into `C:/Users/...` on Windows.
fn from_msys(word: &str) -> String {
    let b = word.as_bytes();
    if cfg!(windows) && b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
        format!("{}:{}", (b[1] as char).to_ascii_uppercase(), &word[2..])
    } else {
        word.to_string()
    }
}

fn looks_like_script(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => SCRIPT_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        // Extensionless files only count when they already exist (shebang scripts).
        None => path.is_file(),
    }
}

/// Script files a command (plus exec-form args) refers to, resolved to
/// absolute paths. Unresolvable references are skipped, not guessed.
pub fn extract_script_paths(command: &str, args: &[String], ctx: &ScriptCtx) -> Vec<PathBuf> {
    let home = fwd(ctx.home);
    let mut text = command.to_string();
    for pat in ["${HOME}", "$HOME", "%USERPROFILE%", "$env:USERPROFILE", "${env:USERPROFILE}"] {
        text = text.replace(pat, &home);
    }
    if let Some(pd) = ctx.project_dir {
        let pd = fwd(pd);
        for pat in [
            "$(git rev-parse --show-toplevel)",
            "${CLAUDE_PROJECT_DIR}",
            "$CLAUDE_PROJECT_DIR",
            "%CLAUDE_PROJECT_DIR%",
            "$env:CLAUDE_PROJECT_DIR",
            "${env:CLAUDE_PROJECT_DIR}",
        ] {
            text = text.replace(pat, &pd);
        }
    }
    if let Some(pr) = ctx.plugin_root {
        let pr = fwd(pr);
        for pat in [
            "${CLAUDE_PLUGIN_ROOT}",
            "$CLAUDE_PLUGIN_ROOT",
            "%CLAUDE_PLUGIN_ROOT%",
            "$env:CLAUDE_PLUGIN_ROOT",
            "${env:CLAUDE_PLUGIN_ROOT}",
        ] {
            text = text.replace(pat, &pr);
        }
    }

    let mut words = shell_words(&text);
    words.extend(args.iter().cloned());

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for word in words {
        if !(word.contains('/') || word.contains('\\')) || word.contains("://") {
            continue;
        }
        // Still has an unresolved variable — can't know where it points.
        if word.contains('$') || word.contains('%') {
            continue;
        }
        let word = match word.strip_prefix("~/").or_else(|| word.strip_prefix("~\\")) {
            Some(rest) => format!("{home}/{rest}"),
            None => from_msys(&word),
        };
        let candidate = PathBuf::from(&word);
        let path = if candidate.is_absolute() {
            candidate
        } else if let Some(base) = ctx.base_dir {
            base.join(candidate)
        } else {
            continue;
        };
        if !looks_like_script(&path) {
            continue;
        }
        let path = PathBuf::from(path.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR));
        // Relative references are only believable when they exist.
        if !word.starts_with('/') && !PathBuf::from(&word).is_absolute() && !path.exists() {
            continue;
        }
        if seen.insert(path_key(&path)) {
            out.push(path);
        }
    }
    out
}

fn handler_scripts(hooks: &Value, ctx: &ScriptCtx) -> Vec<ScriptRef> {
    let mut refs = Vec::new();
    let Some(events) = hooks.as_object() else { return refs };
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else { continue };
        for (gi, group) in groups.iter().enumerate() {
            let Some(handlers) = group.get("hooks").and_then(|h| h.as_array()) else { continue };
            for (hi, handler) in handlers.iter().enumerate() {
                let is_command = handler
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == "command")
                    .unwrap_or(true);
                let Some(command) = handler.get("command").and_then(|c| c.as_str()) else { continue };
                if !is_command {
                    continue;
                }
                let args: Vec<String> = handler
                    .get("args")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                for path in extract_script_paths(command, &args, ctx) {
                    refs.push(ScriptRef {
                        event: event.clone(),
                        group: gi,
                        handler: hi,
                        exists: path.is_file(),
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    refs
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

struct SourceSpec<'a> {
    file: PathBuf,
    file_label: String,
    kind: &'a str,
    editable: bool,
    project_dir: Option<PathBuf>,
    plugin_root: Option<PathBuf>,
    base_dir: Option<PathBuf>,
    inactive_reason: Option<String>,
}

fn object_or_empty(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Object(o)) => Value::Object(o.clone()),
        _ => Value::Object(Map::new()),
    }
}

/// Read a JSON settings-style file (or a hooks.json) into a HookSource.
fn json_source(spec: SourceSpec, home: &Path) -> HookSource {
    let exists = spec.file.is_file();
    let hash = file_hash(&spec.file);
    let (hooks, disable_all, parse_error) = if !exists {
        (Value::Object(Map::new()), false, None)
    } else {
        match fs::read_to_string(&spec.file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(v) => (
                object_or_empty(v.get("hooks")),
                v.get("disableAllHooks").and_then(|b| b.as_bool()).unwrap_or(false),
                None,
            ),
            Err(e) => (Value::Object(Map::new()), false, Some(e)),
        }
    };
    finish_source(spec, home, exists, hash, hooks, disable_all, parse_error)
}

/// A source whose hooks come from YAML frontmatter (skills, subagents).
fn frontmatter_source(spec: SourceSpec, home: &Path) -> Option<HookSource> {
    let text = fs::read_to_string(&spec.file).ok()?;
    let yaml = extract_frontmatter(&text)?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&yaml).ok()?;
    let hooks_yaml = doc.get("hooks")?;
    let hooks: Value = serde_json::to_value(hooks_yaml).ok()?;
    if !hooks.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
        return None;
    }
    let hash = content_hash(text.as_bytes());
    Some(finish_source(spec, home, true, hash, hooks, false, None))
}

fn finish_source(
    spec: SourceSpec,
    home: &Path,
    exists: bool,
    hash: String,
    hooks: Value,
    disable_all: bool,
    parse_error: Option<String>,
) -> HookSource {
    let ctx = ScriptCtx {
        home,
        project_dir: spec.project_dir.as_deref(),
        plugin_root: spec.plugin_root.as_deref(),
        base_dir: spec.base_dir.as_deref(),
    };
    let scripts = handler_scripts(&hooks, &ctx);
    let inactive_reason = spec.inactive_reason.or_else(|| {
        disable_all.then(|| "disableAllHooks is set in this file".to_string())
    });
    HookSource {
        file: strip_verbatim(&spec.file).to_string_lossy().to_string(),
        file_label: spec.file_label,
        kind: spec.kind.to_string(),
        editable: spec.editable && parse_error.is_none(),
        exists,
        hash,
        hooks,
        disable_all_hooks: disable_all,
        inactive_reason,
        project_dir: spec.project_dir.map(|p| strip_verbatim(&p).to_string_lossy().to_string()),
        plugin_root: spec.plugin_root.map(|p| strip_verbatim(&p).to_string_lossy().to_string()),
        scripts,
        parked: Vec::new(),
        parse_error,
    }
}

fn has_hooks(src: &HookSource) -> bool {
    src.hooks.as_object().map(|o| !o.is_empty()).unwrap_or(false)
}

/// Files under a hooks dir (depth-limited), for orphan detection.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != "node_modules")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Referenced scripts of the sources, plus unreferenced files in `hooks_dir`.
fn group_scripts(sources: &[HookSource], hooks_dir: Option<&Path>, editable: bool) -> Vec<ScriptFile> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for src in sources {
        for s in &src.scripts {
            if s.exists && seen.insert(path_key(Path::new(&s.path))) {
                out.push(ScriptFile {
                    name: display_name(Path::new(&s.path), hooks_dir),
                    path: s.path.clone(),
                    referenced: true,
                    editable: editable && src.kind != "plugin" && src.kind != "managed",
                });
            }
        }
    }
    if let Some(dir) = hooks_dir {
        for f in files_in(dir) {
            if seen.insert(path_key(&f)) {
                out.push(ScriptFile {
                    name: display_name(&f, Some(dir)),
                    path: strip_verbatim(&f).to_string_lossy().to_string(),
                    referenced: false,
                    editable,
                });
            }
        }
    }
    out.sort_by(|a, b| b.referenced.cmp(&a.referenced).then(a.name.cmp(&b.name)));
    out
}

fn display_name(path: &Path, base: Option<&Path>) -> String {
    base.and_then(|b| path.strip_prefix(b).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| {
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
}

/// managed-settings.json plus managed-settings.d/*.json, in merge order.
pub(crate) fn managed_settings_files() -> Vec<PathBuf> {
    let mdir = managed_dir();
    let mut files = vec![mdir.join("managed-settings.json")];
    let mut dropins: Vec<PathBuf> = fs::read_dir(mdir.join("managed-settings.d"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .collect()
        })
        .unwrap_or_default();
    dropins.sort();
    files.extend(dropins);
    files
}

pub(crate) fn managed_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    }
}

/// Projects from ~/.claude.json whose `.claude` dir isn't the user one,
/// deduplicated by canonical `.claude` path.
fn distinct_projects(user_claude: &Path) -> Vec<PathBuf> {
    let canon = |p: &Path| path_key(&p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let mut seen = HashSet::new();
    seen.insert(canon(user_claude));
    let mut out = Vec::new();
    for project in claude_project_paths() {
        if seen.insert(canon(&project.join(".claude"))) {
            out.push(project);
        }
    }
    out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    out
}

fn project_label(project: &Path) -> String {
    project
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| project.to_string_lossy().to_string())
}

/// Every settings file a hook may be written to.
pub fn targets() -> Result<Vec<HookTarget>, String> {
    let claude = claude_dir()?;
    let mut out = vec![
        HookTarget {
            file: claude.join("settings.json").to_string_lossy().to_string(),
            label: "User — settings.json".into(),
            project_dir: None,
        },
        HookTarget {
            file: claude.join("settings.local.json").to_string_lossy().to_string(),
            label: "User — settings.local.json".into(),
            project_dir: None,
        },
    ];
    for project in distinct_projects(&claude) {
        let label = project_label(&project);
        let dir = Some(project.to_string_lossy().to_string());
        out.push(HookTarget {
            file: project.join(".claude").join("settings.json").to_string_lossy().to_string(),
            label: format!("{label} — settings.json (shared)"),
            project_dir: dir.clone(),
        });
        out.push(HookTarget {
            file: project.join(".claude").join("settings.local.json").to_string_lossy().to_string(),
            label: format!("{label} — settings.local.json (local)"),
            project_dir: dir,
        });
    }
    Ok(out)
}

fn is_target(file: &Path) -> Result<bool, String> {
    let key = path_key(file);
    Ok(targets()?.iter().any(|t| path_key(Path::new(&t.file)) == key))
}

#[derive(Deserialize)]
struct InstalledPlugins {
    #[serde(default)]
    plugins: std::collections::BTreeMap<String, Vec<InstalledPlugin>>,
}

#[derive(Deserialize)]
struct InstalledPlugin {
    #[serde(rename = "installPath")]
    install_path: String,
    #[serde(default)]
    scope: Option<String>,
}

/// Hook files a plugin declares: the default hooks/hooks.json plus any
/// paths (or inline object) in .claude-plugin/plugin.json's `hooks` field.
fn plugin_hook_specs(root: &Path) -> Vec<(PathBuf, String, Option<Value>)> {
    let mut out: Vec<(PathBuf, String, Option<Value>)> = Vec::new();
    let default = root.join("hooks").join("hooks.json");
    if default.is_file() {
        out.push((default, "hooks/hooks.json".into(), None));
    }
    let manifest = root.join(".claude-plugin").join("plugin.json");
    let Ok(text) = fs::read_to_string(&manifest) else { return out };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return out };
    let mut add_path = |rel: &str| {
        let p = root.join(rel.trim_start_matches("./"));
        if p.is_file() && !out.iter().any(|(q, _, _)| path_key(q) == path_key(&p)) {
            out.push((p, rel.trim_start_matches("./").to_string(), None));
        }
    };
    match v.get("hooks") {
        Some(Value::String(s)) => add_path(s),
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    add_path(s);
                }
            }
        }
        Some(obj @ Value::Object(_)) => {
            // Inline: either {"hooks": {...}} or the events object itself.
            let hooks = obj.get("hooks").cloned().unwrap_or_else(|| obj.clone());
            out.push((manifest.clone(), "plugin.json (inline)".into(), Some(hooks)));
        }
        _ => {}
    }
    out
}

fn plugin_groups(home: &Path, claude: &Path) -> Vec<HookGroup> {
    let Ok(text) = fs::read_to_string(claude.join("plugins").join("installed_plugins.json")) else {
        return Vec::new();
    };
    let Ok(installed) = serde_json::from_str::<InstalledPlugins>(&text) else {
        return Vec::new();
    };
    let off = disabled_plugins();
    let mut groups = Vec::new();
    for (key, installs) in installed.plugins {
        for install in installs {
            let root = PathBuf::from(&install.install_path);
            let mut sources = Vec::new();
            for (file, label, inline) in plugin_hook_specs(&root) {
                let spec = SourceSpec {
                    file: file.clone(),
                    file_label: label,
                    kind: "plugin",
                    editable: false,
                    project_dir: None,
                    plugin_root: Some(root.clone()),
                    base_dir: Some(root.clone()),
                    inactive_reason: off
                        .contains(&key)
                        .then(|| "plugin is disabled (enabledPlugins)".to_string()),
                };
                let src = match inline {
                    None => json_source(spec, home),
                    Some(hooks) => {
                        let hash = file_hash(&file);
                        finish_source(spec, home, true, hash, object_or_empty(Some(&hooks)), false, None)
                    }
                };
                if has_hooks(&src) || src.parse_error.is_some() {
                    sources.push(src);
                }
            }
            if sources.is_empty() {
                continue;
            }
            let scripts = group_scripts(&sources, None, false);
            let scope = install.scope.as_deref().unwrap_or("user");
            groups.push(HookGroup {
                key: format!("plugin:{key}:{}", path_key(&root)),
                kind: "plugin".into(),
                label: if scope == "user" { key.clone() } else { format!("{key} ({scope})") },
                detail: strip_verbatim(&root).to_string_lossy().to_string(),
                sources,
                scripts,
            });
        }
    }
    groups
}

fn frontmatter_group(home: &Path, claude: &Path, settings: &Settings) -> Option<HookGroup> {
    let mut sources = Vec::new();
    if let Ok(skill_groups) = discovery::discover(settings) {
        for group in skill_groups {
            for skill in group.skills {
                let dir = PathBuf::from(&skill.dir);
                let spec = SourceSpec {
                    file: PathBuf::from(&skill.skill_md),
                    file_label: format!("skill: {}", skill.name),
                    kind: "skill",
                    editable: false,
                    project_dir: None,
                    plugin_root: None,
                    base_dir: Some(dir),
                    inactive_reason: Some("runs only after the skill is invoked".into()),
                };
                sources.extend(frontmatter_source(spec, home));
            }
        }
    }
    let mut agent_dirs = vec![(claude.join("agents"), None)];
    for project in distinct_projects(claude) {
        agent_dirs.push((project.join(".claude").join("agents"), Some(project)));
    }
    for (dir, project) in agent_dirs {
        for file in files_in(&dir) {
            if file.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let spec = SourceSpec {
                file: file.clone(),
                file_label: format!("subagent: {name}"),
                kind: "agent",
                editable: false,
                base_dir: project.clone(),
                project_dir: project.clone(),
                plugin_root: None,
                inactive_reason: Some("runs only while the subagent is active".into()),
            };
            sources.extend(frontmatter_source(spec, home));
        }
    }
    if sources.is_empty() {
        return None;
    }
    let scripts = group_scripts(&sources, None, false);
    Some(HookGroup {
        key: "frontmatter".into(),
        kind: "frontmatter".into(),
        label: "Skill & subagent frontmatter".into(),
        detail: "hooks: declared in SKILL.md / agent .md frontmatter (edit the file itself)".into(),
        sources,
        scripts,
    })
}

/// Settings-file group for a `.claude` dir (user or a project).
fn settings_group(
    key: String,
    kind: &str,
    label: String,
    claude_like: &Path,
    project_dir: Option<&Path>,
    home: &Path,
    parked: &[ParkedHook],
) -> Option<HookGroup> {
    let mut sources = Vec::new();
    for (name, src_kind) in [("settings.json", kind), ("settings.local.json", "local")] {
        let file = claude_like.join(name);
        let spec = SourceSpec {
            file: file.clone(),
            file_label: name.into(),
            kind: src_kind,
            editable: true,
            project_dir: project_dir.map(Path::to_path_buf),
            plugin_root: None,
            base_dir: project_dir.map(Path::to_path_buf),
            inactive_reason: None,
        };
        let mut src = json_source(spec, home);
        let key = path_key(&file);
        src.parked = parked.iter().filter(|p| path_key(Path::new(&p.file)) == key).cloned().collect();
        sources.push(src);
    }
    let hooks_dir = claude_like.join("hooks");
    let scripts = group_scripts(&sources, Some(&hooks_dir), true);
    let interesting = sources.iter().any(|s| {
        has_hooks(s) || !s.parked.is_empty() || s.disable_all_hooks || s.parse_error.is_some()
    }) || !scripts.is_empty();
    // The user group always shows (it's where new hooks usually go); a
    // project only when it has something hook-related.
    if !interesting && kind != "user" {
        return None;
    }
    // Hide a missing settings.local.json unless it's the only place parked hooks point.
    sources.retain(|s| s.exists || !s.parked.is_empty() || s.file_label == "settings.json");
    Some(HookGroup {
        key,
        kind: kind.into(),
        label,
        detail: strip_verbatim(claude_like).to_string_lossy().to_string(),
        sources,
        scripts,
    })
}

pub fn overview(settings: &Settings, sidecar: Option<&Path>) -> Result<HooksOverview, String> {
    let home = home_dir()?;
    let claude = claude_dir()?;
    let parked = sidecar.map(load_sidecar).transpose()?.map(|s| s.hooks).unwrap_or_default();
    let mut groups = Vec::new();

    groups.extend(settings_group(
        "user".into(),
        "user",
        "User hooks".into(),
        &claude,
        None,
        &home,
        &parked,
    ));
    for project in distinct_projects(&claude) {
        groups.extend(settings_group(
            format!("project:{}", project.display()),
            "project",
            project_label(&project),
            &project.join(".claude"),
            Some(&project),
            &home,
            &parked,
        ));
    }

    // Managed policy (file-based only; registry-delivered policy isn't read).
    let mdir = managed_dir();
    let managed_sources: Vec<HookSource> = managed_settings_files()
        .into_iter()
        .filter(|f| f.is_file())
        .map(|file| {
            let label = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            json_source(
                SourceSpec {
                    file,
                    file_label: label,
                    kind: "managed",
                    editable: false,
                    project_dir: None,
                    plugin_root: None,
                    base_dir: None,
                    inactive_reason: None,
                },
                &home,
            )
        })
        .filter(|s| has_hooks(s) || s.disable_all_hooks || s.parse_error.is_some())
        .collect();
    if !managed_sources.is_empty() {
        let scripts = group_scripts(&managed_sources, None, false);
        groups.push(HookGroup {
            key: "managed".into(),
            kind: "managed".into(),
            label: "Managed policy".into(),
            detail: format!(
                "{} (read-only; registry-delivered policy isn't shown)",
                mdir.display()
            ),
            sources: managed_sources,
            scripts,
        });
    }

    groups.extend(plugin_groups(&home, &claude));
    groups.extend(frontmatter_group(&home, &claude, settings));

    // Parked hooks whose file isn't part of any group above.
    let shown: HashSet<String> = groups
        .iter()
        .flat_map(|g| g.sources.iter().map(|s| path_key(Path::new(&s.file))))
        .collect();
    let mut orphan_sources: Vec<HookSource> = Vec::new();
    for p in &parked {
        let key = path_key(Path::new(&p.file));
        if shown.contains(&key) {
            continue;
        }
        if let Some(src) = orphan_sources.iter_mut().find(|s| path_key(Path::new(&s.file)) == key) {
            src.parked.push(p.clone());
            continue;
        }
        let file = PathBuf::from(&p.file);
        let mut src = json_source(
            SourceSpec {
                file_label: file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                file,
                kind: "orphaned",
                editable: false,
                project_dir: None,
                plugin_root: None,
                base_dir: None,
                inactive_reason: None,
            },
            &home,
        );
        src.parked.push(p.clone());
        orphan_sources.push(src);
    }
    if !orphan_sources.is_empty() {
        groups.push(HookGroup {
            key: "orphaned".into(),
            kind: "orphaned".into(),
            label: "Disabled hooks from unknown files".into(),
            detail: "Their settings file is no longer a known project — they can only be deleted".into(),
            sources: orphan_sources,
            scripts: Vec::new(),
        });
    }

    Ok(HooksOverview { groups, targets: targets()?, events: EVENTS.to_vec() })
}

/// Paths the file editor may open because of hooks: hooks/agents dirs and
/// every resolved script.
pub fn allowed_paths(settings: &Settings) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(claude) = claude_dir() else { return out };
    out.push(claude.join("hooks"));
    out.push(claude.join("agents"));
    for project in distinct_projects(&claude) {
        out.push(project.join(".claude").join("hooks"));
        out.push(project.join(".claude").join("agents"));
    }
    if let Ok(ov) = overview(settings, None) {
        for g in ov.groups {
            for s in g.scripts {
                out.push(PathBuf::from(s.path));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Validation and writes
// ---------------------------------------------------------------------------

/// Check a `hooks` object against the shapes Claude Code accepts.
pub fn validate_hooks(hooks: &Value) -> Result<(), String> {
    let events = hooks.as_object().ok_or("hooks must be a JSON object")?;
    let mut problems = Vec::new();
    for (event, groups) in events {
        if !EVENTS.contains(&event.as_str()) {
            problems.push(format!("unknown event \"{event}\""));
            continue;
        }
        let Some(groups) = groups.as_array() else {
            problems.push(format!("{event}: must be an array of matcher groups"));
            continue;
        };
        for (gi, group) in groups.iter().enumerate() {
            let at = format!("{event}[{gi}]");
            let Some(group) = group.as_object() else {
                problems.push(format!("{at}: must be an object"));
                continue;
            };
            if let Some(m) = group.get("matcher") {
                if !m.is_string() {
                    problems.push(format!("{at}.matcher: must be a string"));
                }
            }
            let Some(handlers) = group.get("hooks").and_then(|h| h.as_array()) else {
                problems.push(format!("{at}.hooks: must be an array"));
                continue;
            };
            for (hi, h) in handlers.iter().enumerate() {
                let at = format!("{at}.hooks[{hi}]");
                let Some(h) = h.as_object() else {
                    problems.push(format!("{at}: must be an object"));
                    continue;
                };
                let ty = h.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if !HANDLER_TYPES.contains(&ty) {
                    problems.push(format!("{at}.type: must be one of {}", HANDLER_TYPES.join(", ")));
                    continue;
                }
                let required: &[&str] = match ty {
                    "command" => &["command"],
                    "http" => &["url"],
                    "mcp_tool" => &["server", "tool"],
                    _ => &["prompt"],
                };
                for field in required {
                    let ok = h.get(*field).and_then(|v| v.as_str()).map(|s| !s.trim().is_empty());
                    if ok != Some(true) {
                        problems.push(format!("{at}.{field}: required"));
                    }
                }
                if let Some(t) = h.get("timeout") {
                    if !t.as_f64().map(|n| n > 0.0).unwrap_or(false) {
                        problems.push(format!("{at}.timeout: must be a positive number of seconds"));
                    }
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

pub(crate) fn load_object(file: &Path) -> Result<(Map<String, Value>, String), String> {
    match fs::read(file) {
        Ok(bytes) => {
            let hash = content_hash(&bytes);
            let text = String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", file.display()))?;
            if text.trim().is_empty() {
                return Ok((Map::new(), hash));
            }
            match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(o)) => Ok((o, hash)),
                Ok(_) => Err(format!("{} is not a JSON object", file.display())),
                Err(e) => Err(format!("{} is not valid JSON: {e}", file.display())),
            }
        }
        Err(_) => Ok((Map::new(), MISSING_HASH.into())),
    }
}

pub(crate) fn write_object(file: &Path, obj: &Map<String, Value>) -> Result<String, String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(obj).map_err(|e| e.to_string())? + "\n";
    fs::write(file, &text).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    Ok(content_hash(text.as_bytes()))
}

fn check_hash(file: &Path, expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "CONFLICT: {} changed on disk since it was loaded (Claude Code may have updated it). Reload and try again.",
            file.display()
        ))
    }
}

fn guard_target(file: &Path) -> Result<(), String> {
    if is_target(file)? {
        Ok(())
    } else {
        Err(format!("{} is not an editable hooks settings file", file.display()))
    }
}

/// Replace the `hooks` key of a settings file; everything else is untouched.
pub fn set_hooks(file: &Path, expected_hash: &str, hooks: &Value) -> Result<String, String> {
    guard_target(file)?;
    validate_hooks(hooks)?;
    let (mut obj, hash) = load_object(file)?;
    check_hash(file, expected_hash, &hash)?;
    apply_hooks_value(&mut obj, hooks.clone());
    write_object(file, &obj)
}

fn apply_hooks_value(obj: &mut Map<String, Value>, hooks: Value) {
    let empty = hooks.as_object().map(|o| o.is_empty()).unwrap_or(true);
    if empty {
        obj.shift_remove("hooks");
    } else {
        obj.insert("hooks".into(), hooks);
    }
}

pub fn set_disable_all(file: &Path, expected_hash: &str, disabled: bool) -> Result<String, String> {
    guard_target(file)?;
    let (mut obj, hash) = load_object(file)?;
    check_hash(file, expected_hash, &hash)?;
    if disabled {
        obj.insert("disableAllHooks".into(), Value::Bool(true));
    } else {
        obj.shift_remove("disableAllHooks");
    }
    write_object(file, &obj)
}

// ---------------------------------------------------------------------------
// Disabled-hook store (sidecar in the app's config dir)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
pub struct Sidecar {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub hooks: Vec<ParkedHook>,
}

pub fn load_sidecar(path: &Path) -> Result<Sidecar, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("disabled-hooks store {} is corrupt: {e}", path.display())),
        Err(_) => Ok(Sidecar { version: 1, hooks: Vec::new() }),
    }
}

fn save_sidecar(path: &Path, sidecar: &Sidecar) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(sidecar).map_err(|e| e.to_string())? + "\n";
    // Write-then-rename so a crash can't leave a half-written store.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

static PARK_COUNTER: AtomicU64 = AtomicU64::new(0);

fn park_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}-{}", PARK_COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Remove one handler from a settings file and park it in the sidecar.
pub fn disable_hook(
    file: &Path,
    expected_hash: &str,
    event: &str,
    group: usize,
    handler: usize,
    sidecar_path: &Path,
) -> Result<(), String> {
    guard_target(file)?;
    let (mut obj, hash) = load_object(file)?;
    check_hash(file, expected_hash, &hash)?;
    let mut hooks = obj.get("hooks").cloned().unwrap_or(Value::Object(Map::new()));
    let parked = take_handler(&mut hooks, event, group, handler)?;
    let mut sidecar = load_sidecar(sidecar_path)?;
    sidecar.version = 1;
    sidecar.hooks.push(ParkedHook {
        id: park_id(),
        file: strip_verbatim(file).to_string_lossy().to_string(),
        event: event.to_string(),
        matcher: parked.0,
        group_extra: parked.1,
        handler: parked.2,
        disabled_at: now_secs(),
    });
    // Park first: if the settings write then fails, the hook exists twice
    // (recoverable), never zero times.
    save_sidecar(sidecar_path, &sidecar)?;
    apply_hooks_value(&mut obj, hooks);
    if let Err(e) = write_object(file, &obj) {
        sidecar.hooks.pop();
        let _ = save_sidecar(sidecar_path, &sidecar);
        return Err(e);
    }
    Ok(())
}

/// Remove handler `hi` of group `gi` under `event`, pruning emptied
/// containers. Returns (matcher, other group keys, handler).
fn take_handler(
    hooks: &mut Value,
    event: &str,
    gi: usize,
    hi: usize,
) -> Result<(Option<Value>, Map<String, Value>, Value), String> {
    let missing = || format!("hook {event}[{gi}].hooks[{hi}] no longer exists — reload");
    let events = hooks.as_object_mut().ok_or_else(missing)?;
    let groups = events.get_mut(event).and_then(|g| g.as_array_mut()).ok_or_else(missing)?;
    let group = groups.get_mut(gi).and_then(|g| g.as_object_mut()).ok_or_else(missing)?;
    let handlers = group.get_mut("hooks").and_then(|h| h.as_array_mut()).ok_or_else(missing)?;
    if hi >= handlers.len() {
        return Err(missing());
    }
    let handler = handlers.remove(hi);
    let now_empty = handlers.is_empty();
    let matcher = group.get("matcher").cloned();
    let extra: Map<String, Value> = group
        .iter()
        .filter(|(k, _)| k.as_str() != "hooks" && k.as_str() != "matcher")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if now_empty {
        groups.remove(gi);
    }
    if groups.is_empty() {
        events.shift_remove(event);
    }
    Ok((matcher, extra, handler))
}

/// Put a handler back: into the first group with the same matcher, or a new group.
fn insert_handler(hooks: &mut Value, parked: &ParkedHook) {
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let events = hooks.as_object_mut().expect("object");
    let groups = events
        .entry(parked.event.clone())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !groups.is_array() {
        *groups = Value::Array(Vec::new());
    }
    let groups = groups.as_array_mut().expect("array");
    let same = groups.iter_mut().find(|g| {
        g.get("matcher") == parked.matcher.as_ref()
            && g.get("hooks").map(|h| h.is_array()).unwrap_or(false)
    });
    match same {
        Some(g) => g["hooks"].as_array_mut().expect("array").push(parked.handler.clone()),
        None => {
            let mut group = Map::new();
            if let Some(m) = &parked.matcher {
                group.insert("matcher".into(), m.clone());
            }
            for (k, v) in &parked.group_extra {
                group.insert(k.clone(), v.clone());
            }
            group.insert("hooks".into(), Value::Array(vec![parked.handler.clone()]));
            groups.push(Value::Object(group));
        }
    }
}

pub fn enable_hook(id: &str, sidecar_path: &Path) -> Result<(), String> {
    let mut sidecar = load_sidecar(sidecar_path)?;
    let idx = sidecar
        .hooks
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| format!("disabled hook {id} not found"))?;
    let parked = sidecar.hooks[idx].clone();
    let file = PathBuf::from(&parked.file);
    guard_target(&file)?;
    let (mut obj, _) = load_object(&file)?;
    let mut hooks = obj.get("hooks").cloned().unwrap_or(Value::Object(Map::new()));
    insert_handler(&mut hooks, &parked);
    validate_hooks(&hooks)?;
    apply_hooks_value(&mut obj, hooks);
    write_object(&file, &obj)?;
    sidecar.hooks.remove(idx);
    save_sidecar(sidecar_path, &sidecar)
}

pub fn delete_parked(id: &str, sidecar_path: &Path) -> Result<(), String> {
    let mut sidecar = load_sidecar(sidecar_path)?;
    let before = sidecar.hooks.len();
    sidecar.hooks.retain(|p| p.id != id);
    if sidecar.hooks.len() == before {
        return Err(format!("disabled hook {id} not found"));
    }
    save_sidecar(sidecar_path, &sidecar)
}

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TestRequest {
    pub command: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<f64>,
    #[serde(default)]
    pub project_dir: Option<String>,
    #[serde(default)]
    pub plugin_root: Option<String>,
    pub stdin: String,
}

#[derive(Serialize)]
pub struct TestResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// How the hook was launched, for display.
    pub runner: String,
}

/// Git Bash, which is what Claude Code runs `shell: "bash"` hooks with on
/// Windows (never WSL's System32 bash).
fn git_bash() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if !cfg!(windows) {
        return Some(PathBuf::from("bash"));
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if !dir.join("git.exe").is_file() {
            continue;
        }
        for ancestor in dir.ancestors().take(4) {
            let bash = ancestor.join("bin").join("bash.exe");
            if bash.is_file() {
                return Some(bash);
            }
        }
    }
    [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn powershell() -> PathBuf {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        for name in ["pwsh.exe", "pwsh"] {
            if dir.join(name).is_file() {
                return dir.join(name);
            }
        }
    }
    PathBuf::from(if cfg!(windows) { "powershell.exe" } else { "pwsh" })
}

const OUTPUT_CAP: usize = 256 * 1024;

type SharedBuf = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

/// Read a pipe into a shared buffer on a thread. The buffer is readable even
/// if the thread never finishes (a grandchild can hold the pipe open).
fn drain<R: Read + Send + 'static>(mut r: R) -> (SharedBuf, std::thread::JoinHandle<()>) {
    let buf: SharedBuf = Default::default();
    let sink = buf.clone();
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut b = sink.lock().unwrap();
                    if b.len() < OUTPUT_CAP {
                        let take = n.min(OUTPUT_CAP - b.len());
                        b.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
    });
    (buf, handle)
}

fn buf_text(buf: &SharedBuf) -> String {
    let b = buf.lock().unwrap();
    let mut s = String::from_utf8_lossy(&b).to_string();
    if b.len() >= OUTPUT_CAP {
        s.push_str("\n… [output truncated]");
    }
    s
}

/// Kill a hook and everything it started: a shell usually has children, and
/// Git Bash's bash.exe is itself a wrapper around a second process.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let mut tk = Command::new("taskkill");
        tk.args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = hide_console(&mut tk).status();
    }
    // Kill the child's own children by parent PID. Never signal a process
    // GROUP here (`kill -- -PID`): if the child isn't the group leader we
    // would be signalling whoever owns that group — on a CI runner, that
    // took down the agent itself.
    #[cfg(unix)]
    {
        let mut pkill = Command::new("pkill");
        let _ = pkill
            .args(["-KILL", "-P", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub fn run_test(req: TestRequest) -> Result<TestResult, String> {
    let home = home_dir()?;
    let project_dir = req.project_dir.as_deref().filter(|s| !s.is_empty()).map(PathBuf::from);
    let cwd = project_dir.clone().unwrap_or_else(|| home.clone());
    let substitute = |s: &str| {
        let mut s = s.to_string();
        if let Some(pd) = &project_dir {
            s = s.replace("${CLAUDE_PROJECT_DIR}", &fwd(pd));
        }
        if let Some(pr) = req.plugin_root.as_deref() {
            s = s.replace("${CLAUDE_PLUGIN_ROOT}", &fwd(Path::new(pr)));
        }
        s
    };
    let command = substitute(&req.command);

    let (mut cmd, runner) = match &req.args {
        Some(args) => {
            let mut c = Command::new(&command);
            c.args(args.iter().map(|a| substitute(a)));
            (c, format!("exec {command}"))
        }
        None if req.shell.as_deref() == Some("powershell") => {
            let ps = powershell();
            let mut c = Command::new(&ps);
            c.args(["-NoProfile", "-NonInteractive", "-Command", &command]);
            (c, format!("{} -Command", ps.display()))
        }
        None => {
            let bash = git_bash().ok_or(
                "Git Bash not found — install Git for Windows or set CLAUDE_CODE_GIT_BASH_PATH",
            )?;
            let mut c = Command::new(&bash);
            c.args(["-c", &command]);
            (c, format!("{} -c", bash.display()))
        }
    };
    cmd.current_dir(&cwd)
        .env("CLAUDE_PROJECT_DIR", &cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pr) = &req.plugin_root {
        cmd.env("CLAUDE_PLUGIN_ROOT", pr);
    }
    // Its own process group, so a timeout can take the shell's children with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let started = Instant::now();
    let mut child = hide_console(&mut cmd)
        .spawn()
        .map_err(|e| format!("failed to start hook ({runner}): {e}"))?;
    let out = drain(child.stdout.take().expect("piped"));
    let err = drain(child.stderr.take().expect("piped"));
    if let Some(mut stdin) = child.stdin.take() {
        let input = req.stdin.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }

    let timeout = Duration::from_secs_f64(req.timeout_secs.filter(|t| *t > 0.0).unwrap_or(60.0));
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                kill_tree(&mut child);
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("failed waiting for hook: {e}")),
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;

    // Give the readers a moment to collect trailing output, but don't wait on
    // a background process that inherited the pipes.
    let grace = Instant::now();
    while !(out.1.is_finished() && err.1.is_finished()) && grace.elapsed() < Duration::from_millis(1500) {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(TestResult {
        exit_code: if timed_out { None } else { status.and_then(|s| s.code()) },
        stdout: buf_text(&out.0),
        stderr: buf_text(&err.0),
        timed_out,
        duration_ms,
        runner,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("skills-editor-hooks-{tag}-{}", park_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Prints what discovery finds on the real machine:
    /// `cargo test live_overview -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_overview() {
        let ov = overview(&Settings::default(), None).unwrap();
        for g in &ov.groups {
            println!("[{}] {} — {}", g.kind, g.label, g.detail);
            for s in &g.sources {
                let events: Vec<String> = s
                    .hooks
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .map(|(k, v)| format!("{k}×{}", v.as_array().map(|a| a.len()).unwrap_or(0)))
                            .collect()
                    })
                    .unwrap_or_default();
                println!(
                    "   {} exists={} editable={} inactive={:?} events={:?} scripts={}",
                    s.file_label,
                    s.exists,
                    s.editable,
                    s.inactive_reason,
                    events,
                    s.scripts.len()
                );
            }
            for sc in &g.scripts {
                println!("   script {} referenced={}", sc.name, sc.referenced);
            }
        }
        println!("targets: {}", ov.targets.len());
    }

    #[test]
    fn shell_words_split_on_quotes_and_operators() {
        assert_eq!(
            shell_words(r#"bash "a b/c.sh" && echo 'x y'|cat"#),
            vec!["bash", "a b/c.sh", "echo", "x y", "cat"]
        );
    }

    #[test]
    fn resolves_home_project_and_plugin_scripts() {
        let home = temp_dir("home");
        let project = temp_dir("proj");
        let plugin = temp_dir("plug");
        fs::create_dir_all(project.join(".claude/hooks")).unwrap();
        fs::write(project.join(".claude/hooks/check.sh"), "#!/bin/sh").unwrap();
        let ctx = ScriptCtx {
            home: &home,
            project_dir: Some(&project),
            plugin_root: Some(&plugin),
            base_dir: Some(&project),
        };

        let got = extract_script_paths("bash ~/.claude/hooks/block.sh", &[], &ctx);
        assert_eq!(got, vec![home.join(".claude").join("hooks").join("block.sh")]);

        let got = extract_script_paths(
            r#"bash "$(git rev-parse --show-toplevel)/.claude/hooks/check.sh""#,
            &[],
            &ctx,
        );
        assert_eq!(path_key(&got[0]), path_key(&project.join(".claude/hooks/check.sh")));

        let got = extract_script_paths("${CLAUDE_PLUGIN_ROOT}/scripts/v.py --x", &[], &ctx);
        assert_eq!(path_key(&got[0]), path_key(&plugin.join("scripts/v.py")));

        // Relative path: only when it exists.
        let got = extract_script_paths("sh .claude/hooks/check.sh", &[], &ctx);
        assert_eq!(got.len(), 1);
        assert!(extract_script_paths("sh .claude/hooks/nope.sh", &[], &ctx).is_empty());

        // Exec-form args count too.
        let got = extract_script_paths("node", &["~/tools/hook.mjs".to_string()], &ctx);
        assert_eq!(got, vec![home.join("tools").join("hook.mjs")]);

        for d in [home, project, plugin] {
            let _ = fs::remove_dir_all(d);
        }
    }

    #[test]
    fn ignores_non_scripts_and_unresolvable_references() {
        let home = temp_dir("home2");
        let ctx = ScriptCtx { home: &home, project_dir: None, plugin_root: None, base_dir: None };
        // Executables, URLs, unresolved variables, relative paths without a base.
        assert!(extract_script_paths(r#""C:/Tools/gk.exe" ai hook run"#, &[], &ctx).is_empty());
        assert!(extract_script_paths("curl https://x.example/hook.sh", &[], &ctx).is_empty());
        assert!(extract_script_paths("bash $CLAUDE_PROJECT_DIR/h.sh", &[], &ctx).is_empty());
        assert!(extract_script_paths("bash scripts/h.sh", &[], &ctx).is_empty());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn validation_catches_bad_shapes() {
        assert!(validate_hooks(&json!({
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "x", "timeout": 5 }] }],
            "Stop": [{ "hooks": [{ "type": "agent", "prompt": "check" }] }]
        }))
        .is_ok());
        let err = validate_hooks(&json!({
            "PreToolUsee": [],
            "Stop": [{ "hooks": [{ "type": "command" }, { "type": "bogus" }] }],
            "PostToolUse": [{ "matcher": 3, "hooks": [{ "type": "http", "url": "u", "timeout": 0 }] }]
        }))
        .unwrap_err();
        for needle in ["unknown event \"PreToolUsee\"", "command: required", "type: must be one of", "matcher: must be a string", "timeout: must be"] {
            assert!(err.contains(needle), "missing {needle:?} in {err}");
        }
    }

    #[test]
    fn take_and_insert_round_trip_prunes_and_restores() {
        let mut hooks = json!({
            "PreToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "a" }] },
                { "matcher": "Edit", "hooks": [{ "type": "command", "command": "b" }, { "type": "command", "command": "c" }] }
            ],
            "Stop": [{ "hooks": [{ "type": "prompt", "prompt": "p" }] }]
        });
        let (matcher, extra, handler) = take_handler(&mut hooks, "PreToolUse", 0, 0).unwrap();
        assert_eq!(matcher, Some(json!("Bash")));
        assert!(extra.is_empty());
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 1, "emptied group pruned");

        let parked = ParkedHook {
            id: "x".into(),
            file: String::new(),
            event: "PreToolUse".into(),
            matcher,
            group_extra: extra,
            handler,
            disabled_at: 0,
        };
        insert_handler(&mut hooks, &parked);
        assert_eq!(hooks["PreToolUse"][1], json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": "a" }] }));

        // Taking the last handler of the last group removes the event.
        take_handler(&mut hooks, "Stop", 0, 0).unwrap();
        assert!(hooks.get("Stop").is_none());
        // Stale indices are an error, not a panic.
        assert!(take_handler(&mut hooks, "Stop", 0, 0).is_err());
        assert!(take_handler(&mut hooks, "PreToolUse", 0, 5).is_err());
    }

    #[test]
    fn insert_joins_existing_matcher_group() {
        let mut hooks = json!({ "Stop": [{ "hooks": [{ "type": "prompt", "prompt": "1" }] }] });
        let parked = ParkedHook {
            id: "x".into(),
            file: String::new(),
            event: "Stop".into(),
            matcher: None,
            group_extra: Map::new(),
            handler: json!({ "type": "prompt", "prompt": "2" }),
            disabled_at: 0,
        };
        insert_handler(&mut hooks, &parked);
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["Stop"][0]["hooks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn writes_keep_other_keys_in_order_and_detect_conflicts() {
        let dir = temp_dir("write");
        let file = dir.join("settings.json");
        fs::write(&file, "{\n  \"a\": 1,\n  \"hooks\": {},\n  \"z\": 2\n}\n").unwrap();
        let (mut obj, hash) = load_object(&file).unwrap();
        check_hash(&file, &hash, &hash).unwrap();

        apply_hooks_value(&mut obj, json!({ "Stop": [{ "hooks": [{ "type": "prompt", "prompt": "p" }] }] }));
        let new_hash = write_object(&file, &obj).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        let keys: Vec<&str> = ["\"a\"", "\"hooks\"", "\"z\""].to_vec();
        let pos: Vec<usize> = keys.iter().map(|k| text.find(k).unwrap()).collect();
        assert!(pos[0] < pos[1] && pos[1] < pos[2], "order preserved: {text}");
        assert_eq!(new_hash, file_hash(&file));

        // Emptying hooks removes the key without disturbing the rest.
        let (mut obj, _) = load_object(&file).unwrap();
        apply_hooks_value(&mut obj, json!({}));
        write_object(&file, &obj).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("hooks"));
        assert!(text.find("\"a\"").unwrap() < text.find("\"z\"").unwrap());

        // Someone else edits the file → the stale hash is rejected.
        fs::write(&file, "{\"a\": 3}").unwrap();
        let (_, current) = load_object(&file).unwrap();
        let err = check_hash(&file, &new_hash, &current).unwrap_err();
        assert!(err.starts_with("CONFLICT"));

        // Missing files load as empty with the sentinel hash.
        let (obj, hash) = load_object(&dir.join("nope.json")).unwrap();
        assert!(obj.is_empty());
        assert_eq!(hash, MISSING_HASH);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sidecar_round_trip() {
        let dir = temp_dir("sidecar");
        let path = dir.join("disabled-hooks.json");
        assert!(load_sidecar(&path).unwrap().hooks.is_empty());
        let sc = Sidecar {
            version: 1,
            hooks: vec![ParkedHook {
                id: park_id(),
                file: "f".into(),
                event: "Stop".into(),
                matcher: None,
                group_extra: Map::new(),
                handler: json!({ "type": "prompt", "prompt": "p" }),
                disabled_at: 1,
            }],
        };
        save_sidecar(&path, &sc).unwrap();
        let loaded = load_sidecar(&path).unwrap();
        assert_eq!(loaded.hooks.len(), 1);
        delete_parked(&loaded.hooks[0].id, &path).unwrap();
        assert!(load_sidecar(&path).unwrap().hooks.is_empty());
        assert!(delete_parked("nope", &path).is_err());
        fs::write(&path, "not json").unwrap();
        assert!(load_sidecar(&path).is_err(), "corrupt store must not read as empty");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn frontmatter_hooks_are_read() {
        let dir = temp_dir("fm");
        let md = dir.join("SKILL.md");
        fs::write(
            &md,
            "---\nname: s\nhooks:\n  PreToolUse:\n    - matcher: Bash\n      hooks:\n        - type: command\n          command: ./check.sh\n          once: true\n---\nbody\n",
        )
        .unwrap();
        let spec = SourceSpec {
            file: md,
            file_label: "SKILL.md".into(),
            kind: "skill",
            editable: false,
            project_dir: None,
            plugin_root: None,
            base_dir: Some(dir.clone()),
            inactive_reason: None,
        };
        let src = frontmatter_source(spec, &dir).unwrap();
        assert_eq!(src.hooks["PreToolUse"][0]["hooks"][0]["once"], json!(true));
        let _ = fs::remove_dir_all(dir);
    }

    /// The full write path through the public operations, against a fake
    /// home with one registered project.
    #[test]
    fn end_to_end_create_disable_enable_delete() {
        let home = temp_dir("e2e-home");
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let project = home.join("proj");
        fs::create_dir_all(project.join(".claude").join("hooks")).unwrap();
        fs::write(project.join(".claude/hooks/guard.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(
            home.join(".claude.json"),
            json!({ "projects": { (project.to_string_lossy().replace('\\', "/")): {} } }).to_string(),
        )
        .unwrap();
        let user_settings = home.join(".claude").join("settings.json");
        fs::write(&user_settings, "{\n  \"model\": \"opus\",\n  \"permissions\": {}\n}\n").unwrap();
        let local = project.join(".claude").join("settings.local.json");
        let sidecar = home.join("app").join("disabled-hooks.json");
        let settings = Settings::default();
        let source = |file: &Path| -> HookSource {
            overview(&settings, Some(&sidecar))
                .unwrap()
                .groups
                .into_iter()
                .flat_map(|g| g.sources)
                .find(|s| path_key(Path::new(&s.file)) == path_key(file))
                .expect("source listed")
        };

        // Targets include the fake project's (not yet existing) local file.
        let targets = targets().unwrap();
        assert!(targets.iter().any(|t| path_key(Path::new(&t.file)) == path_key(&local)));
        // Files outside the known targets are refused.
        assert!(set_hooks(&home.join("elsewhere.json"), MISSING_HASH, &json!({})).is_err());

        // Create hooks in the project's local file (doesn't exist yet).
        let hooks = json!({
            "PreToolUse": [{ "matcher": "Bash", "hooks": [
                { "type": "command", "command": "bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.sh\"" },
                { "type": "command", "command": "echo second" }
            ]}]
        });
        assert!(set_hooks(&local, "wrong", &hooks).unwrap_err().starts_with("CONFLICT"));
        set_hooks(&local, MISSING_HASH, &hooks).unwrap();
        let src = source(&local);
        assert_eq!(src.kind, "local");
        assert_eq!(src.scripts.len(), 1, "guard.sh resolved via CLAUDE_PROJECT_DIR");
        assert!(src.scripts[0].exists);

        // Disable the first handler: it leaves the file and is parked.
        disable_hook(&local, &src.hash, "PreToolUse", 0, 0, &sidecar).unwrap();
        let src = source(&local);
        assert_eq!(src.hooks["PreToolUse"][0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(src.parked.len(), 1);
        // A stale hash can't disable again.
        assert!(disable_hook(&local, "stale", "PreToolUse", 0, 0, &sidecar).is_err());

        // Enable it: back into the Bash group, sidecar emptied.
        enable_hook(&src.parked[0].id, &sidecar).unwrap();
        let src = source(&local);
        assert_eq!(src.hooks["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(src.hooks["PreToolUse"][0]["hooks"].as_array().unwrap().len(), 2);
        assert!(src.parked.is_empty());

        // disableAllHooks round trip leaves other keys and marks the source inactive.
        let hash = set_disable_all(&user_settings, &source(&user_settings).hash, true).unwrap();
        let user = source(&user_settings);
        assert!(user.disable_all_hooks && user.inactive_reason.is_some());
        set_disable_all(&user_settings, &hash, false).unwrap();
        let text = fs::read_to_string(&user_settings).unwrap();
        assert_eq!(text, "{\n  \"model\": \"opus\",\n  \"permissions\": {}\n}\n", "byte-identical round trip");

        // Clearing the hooks removes the key entirely.
        set_hooks(&local, &source(&local).hash, &json!({})).unwrap();
        assert_eq!(fs::read_to_string(&local).unwrap().trim(), "{}");

        // The unreferenced script still shows up for the project.
        let ov = overview(&settings, Some(&sidecar)).unwrap();
        let proj = ov.groups.iter().find(|g| g.kind == "project").expect("project group");
        assert_eq!(proj.scripts.len(), 1);
        assert!(!proj.scripts[0].referenced);

        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = None);
    }

    #[test]
    fn test_runner_reports_exit_codes_output_and_timeouts() {
        if git_bash().is_none() {
            eprintln!("skipping: no bash available");
            return;
        }
        let ok = run_test(TestRequest {
            command: "read -r line; echo \"got:$line\"; echo warn >&2; exit 2".into(),
            args: None,
            shell: Some("bash".into()),
            timeout_secs: Some(20.0),
            project_dir: None,
            plugin_root: None,
            stdin: "{\"hook_event_name\":\"Stop\"}\n".into(),
        })
        .unwrap();
        assert_eq!(ok.exit_code, Some(2));
        assert_eq!(ok.stdout.trim(), "got:{\"hook_event_name\":\"Stop\"}");
        assert_eq!(ok.stderr.trim(), "warn");
        assert!(!ok.timed_out);

        let slow = run_test(TestRequest {
            command: "sleep 5".into(),
            args: None,
            shell: None,
            timeout_secs: Some(0.5),
            project_dir: None,
            plugin_root: None,
            stdin: String::new(),
        })
        .unwrap();
        assert!(slow.timed_out);
        assert_eq!(slow.exit_code, None);
        assert!(slow.duration_ms < 4000);
    }
}
