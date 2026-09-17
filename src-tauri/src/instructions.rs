//! CLAUDE.md-style instruction files, rules, and other agents' instruction
//! files — the configuration you write for Claude. The notes Claude writes
//! for itself live in `memory.rs`. See plans/instructions.md.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::discovery::{claude_project_paths, extract_frontmatter};
use crate::hooks::{load_object, managed_dir, managed_settings_files, write_object};
use crate::paths::{claude_dir, home_dir, path_key, strip_verbatim};

/// Claude Code skips larger instruction files entirely.
const MAX_FILE: u64 = 4 * 1024 * 1024;
/// Only this much of MEMORY.md loads at session start.
pub(crate) const MEMORY_MAX_LINES: usize = 200;
pub(crate) const MEMORY_MAX_BYTES: usize = 25 * 1024;
/// Docs guidance for CLAUDE.md length.
const RECOMMENDED_LINES: usize = 200;
const IMPORT_DEPTH: usize = 4;
const NESTED_DEPTH: usize = 8;
const NESTED_BUDGET: usize = 50_000;
const SKIP_DIRS: [&str; 11] = [
    ".git", "node_modules", "target", "dist", "build", ".venv", "venv", "__pycache__", ".next",
    ".turbo", ".t3",
];

pub const CLAUDE_NAMES: [&str; 2] = ["CLAUDE.md", "CLAUDE.local.md"];

/// Other agents' instruction files relative to a project: (path, agent).
const OTHER_AGENT_FILES: [(&str, &str); 6] = [
    ("AGENTS.md", "Codex etc."),
    ("GEMINI.md", "Gemini CLI"),
    (".github/copilot-instructions.md", "GitHub Copilot"),
    (".cursorrules", "Cursor"),
    (".windsurfrules", "Windsurf"),
    (".clinerules", "Cline"),
];
/// Directories of rule files other agents use: (dir, agent).
const OTHER_AGENT_DIRS: [(&str, &str); 3] = [
    (".cursor/rules", "Cursor"),
    (".windsurf/rules", "Windsurf"),
    (".clinerules", "Cline"),
];
/// User-level files of other agents, relative to home.
const USER_AGENT_FILES: [(&str, &str); 2] = [
    (".codex/AGENTS.md", "Codex"),
    (".gemini/GEMINI.md", "Gemini CLI"),
];

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct ImportRef {
    /// The text after `@`, as written.
    pub raw: String,
    pub path: String,
    pub exists: bool,
    /// Resolves outside the project: needs a one-time approval in Claude Code.
    pub external: bool,
}

#[derive(Serialize, Clone)]
pub struct InstrFile {
    pub path: String,
    /// Display name relative to the group's root.
    pub label: String,
    /// "claude" | "local" | "rule" | "nested" | "other-agent"
    pub kind: String,
    /// "startup" | "on-demand" | "imported" | "never"
    pub loads: String,
    pub editable: bool,
    pub bytes: u64,
    pub lines: usize,
    pub excluded: bool,
    /// `paths:` globs of a rule (on-demand when present).
    pub paths: Vec<String>,
    pub agent: Option<String>,
    /// Claude instruction files that `@import` this one.
    pub imported_by: Vec<String>,
    /// For ancestor files: the projects they apply to.
    pub applies_to: Vec<String>,
    pub imports: Vec<ImportRef>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct StartupEntry {
    /// Empty for inline content (managed `claudeMd`).
    pub path: String,
    pub label: String,
    /// Import nesting (0 = loaded directly).
    pub depth: usize,
    pub bytes: u64,
    /// What actually enters context (MEMORY.md is truncated).
    pub loaded_bytes: u64,
    pub lines: usize,
    pub note: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct AutoMemory {
    pub dir: String,
    pub exists: bool,
    pub enabled: bool,
    /// Where the enabled state comes from, for display.
    pub source: String,
    pub custom_dir: bool,
}

#[derive(Serialize, Clone)]
pub struct InstrGroup {
    pub key: String,
    /// "managed" | "user" | "project" | "parents"
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub project_dir: Option<String>,
    pub files: Vec<InstrFile>,
    pub startup: Vec<StartupEntry>,
    pub auto_memory: Option<AutoMemory>,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct ProjectRef {
    pub dir: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct InstrOverview {
    pub groups: Vec<InstrGroup>,
    /// Every registered project that exists, for the create dialog.
    pub projects: Vec<ProjectRef>,
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

pub(crate) fn s(p: &Path) -> String {
    strip_verbatim(p).to_string_lossy().to_string()
}

fn fwd(p: &Path) -> String {
    s(p).replace('\\', "/")
}

/// Lexically resolve `.` and `..` (paths may not exist yet).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when `path` is `root` or inside it (case/separator-insensitive).
pub(crate) fn under(path: &Path, root: &Path) -> bool {
    let p = path_key(path);
    let r = path_key(root);
    let sep = if cfg!(windows) { '\\' } else { '/' };
    p == r || p.starts_with(&format!("{r}{sep}"))
}

pub(crate) fn canon(p: &Path) -> PathBuf {
    p.canonicalize().map(|c| strip_verbatim(&c)).unwrap_or_else(|_| normalize(p))
}

/// The auto-memory directory name Claude Code derives from a project path.
pub fn encode_project(p: &Path) -> String {
    s(p).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Nearest ancestor (or self) containing `.git`, else the path itself.
pub(crate) fn git_root(p: &Path) -> PathBuf {
    p.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| p.to_path_buf())
}

pub(crate) struct Content {
    pub bytes: u64,
    pub lines: usize,
    pub text: Option<String>,
}

pub(crate) fn read_content(path: &Path) -> Content {
    let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if bytes > MAX_FILE {
        return Content { bytes, lines: 0, text: None };
    }
    let text = fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let lines = text.as_deref().map(|t| t.lines().count()).unwrap_or(0);
    Content { bytes, lines, text }
}

pub(crate) fn frontmatter(text: &str) -> Option<serde_yaml::Value> {
    serde_yaml::from_str(&extract_frontmatter(text)?).ok()
}

pub(crate) fn yaml_str(v: &serde_yaml::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

fn rule_paths(text: &str) -> Vec<String> {
    let Some(fm) = frontmatter(text) else { return Vec::new() };
    match fm.get("paths") {
        Some(serde_yaml::Value::String(one)) => vec![one.clone()],
        Some(serde_yaml::Value::Sequence(items)) => {
            items.iter().filter_map(|i| i.as_str().map(str::to_string)).collect()
        }
        _ => Vec::new(),
    }
}

/// Bytes of MEMORY.md that load: the first 200 lines, capped at 25 KB.
pub(crate) fn memory_loaded_bytes(text: &str) -> u64 {
    let mut end = 0usize;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i >= MEMORY_MAX_LINES || end + line.len() > MEMORY_MAX_BYTES {
            break;
        }
        end += line.len();
    }
    end as u64
}

// ---------------------------------------------------------------------------
// Settings values
// ---------------------------------------------------------------------------

fn settings_value(file: &Path, key: &str) -> Option<Value> {
    let text = fs::read_to_string(file).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get(key).cloned()
}

fn settings_strings(file: &Path, key: &str) -> Vec<String> {
    match settings_value(file, key) {
        Some(Value::Array(a)) => a.into_iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// A settings scope: (settings.json, settings.local.json) of a `.claude` dir.
pub(crate) fn scope_files(claude_like: &Path) -> [PathBuf; 2] {
    [claude_like.join("settings.json"), claude_like.join("settings.local.json")]
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    match value.strip_prefix("~/").or_else(|| value.strip_prefix("~\\")) {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    }
}

/// `claudeMdExcludes` globs, matched against absolute paths.
struct Excludes(Option<GlobSet>);

impl Excludes {
    fn new(patterns: &[String]) -> Excludes {
        let mut b = GlobSetBuilder::new();
        let mut any = false;
        for p in patterns {
            if let Ok(g) = GlobBuilder::new(&p.replace('\\', "/"))
                .literal_separator(true)
                .case_insensitive(cfg!(windows))
                .build()
            {
                b.add(g);
                any = true;
            }
        }
        Excludes(if any { b.build().ok() } else { None })
    }

    fn matches(&self, path: &Path) -> bool {
        self.0.as_ref().map(|g| g.is_match(fwd(path))).unwrap_or(false)
    }
}

fn exclude_patterns(files: &[PathBuf]) -> Vec<String> {
    files.iter().flat_map(|f| settings_strings(f, "claudeMdExcludes")).collect()
}

#[cfg(not(test))]
fn env_disables_auto_memory() -> bool {
    std::env::var("CLAUDE_CODE_DISABLE_AUTO_MEMORY")
        .map(|v| !v.is_empty() && v != "0" && v.to_lowercase() != "false")
        .unwrap_or(false)
}

#[cfg(test)]
fn env_disables_auto_memory() -> bool {
    false
}

/// Effective auto-memory state for a list of settings files, highest
/// precedence first, each labelled for display.
pub(crate) fn auto_memory_state(
    layers: &[(PathBuf, &str)],
    default_dir: PathBuf,
    home: &Path,
) -> AutoMemory {
    let (enabled, source) = if env_disables_auto_memory() {
        (false, "CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string())
    } else {
        layers
            .iter()
            .find_map(|(f, label)| {
                settings_value(f, "autoMemoryEnabled")
                    .and_then(|v| v.as_bool())
                    .map(|b| (b, label.to_string()))
            })
            .unwrap_or((true, "default".into()))
    };
    let custom = layers.iter().find_map(|(f, _)| {
        settings_value(f, "autoMemoryDirectory")
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|v| v.starts_with("~/") || Path::new(v).is_absolute())
    });
    let (dir, custom_dir) = match custom {
        Some(v) => (expand_home(&v, home), true),
        None => (default_dir, false),
    };
    AutoMemory { exists: dir.is_dir(), dir: s(&dir), enabled, source, custom_dir }
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

fn import_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?:^|[\s(\[])@([^\s`'"()\[\]<>]+)"#).expect("valid regex"))
}

fn code_span_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`+[^`]*`+").expect("valid regex"))
}

/// The `@path` references of a file, ignoring fenced blocks and code spans.
pub fn parse_imports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                continue;
            }
            (Some(open), Some(m)) if m == open => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            _ => {}
        }
        let plain = code_span_re().replace_all(line, " ");
        for cap in import_re().captures_iter(&plain) {
            let raw = cap[1].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            if !raw.is_empty() && !out.iter().any(|r| r == raw) {
                out.push(raw.to_string());
            }
        }
    }
    out
}

/// Worth reporting as a broken import even though the file is missing.
fn looks_like_path(raw: &str) -> bool {
    let b = raw.as_bytes();
    raw.starts_with("~/")
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with('/')
        || (b.len() > 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
        || raw
            .rsplit(['/', '\\'])
            .next()
            .map(|last| last.contains('.') && !last.ends_with('.'))
            .unwrap_or(false)
}

fn resolve_import(raw: &str, from: &Path, home: &Path) -> PathBuf {
    let p = if raw.starts_with("~/") || raw.starts_with("~\\") {
        expand_home(raw, home)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        from.parent().unwrap_or(Path::new("")).join(raw)
    };
    normalize(&PathBuf::from(p.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn imports_of(file: &Path, text: &str, scope: Option<&Path>, home: &Path) -> Vec<ImportRef> {
    parse_imports(text)
        .into_iter()
        .filter_map(|raw| {
            let path = resolve_import(&raw, file, home);
            let exists = path.is_file();
            if !exists && !looks_like_path(&raw) {
                return None;
            }
            Some(ImportRef {
                external: scope.map(|sc| !under(&path, sc)).unwrap_or(false),
                path: s(&path),
                exists,
                raw,
            })
        })
        .collect()
}

fn import_cache() -> &'static Mutex<HashSet<String>> {
    static CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

// ---------------------------------------------------------------------------
// Building file entries
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    home: &'a Path,
    /// Project root for external-import detection.
    scope: Option<&'a Path>,
    excludes: &'a Excludes,
}

fn build_file(path: &Path, label: String, kind: &str, editable: bool, ctx: &Ctx) -> InstrFile {
    let content = read_content(path);
    let text = content.text.as_deref().unwrap_or("");
    let mut warnings = Vec::new();
    let mut loads = match kind {
        "claude" | "local" => "startup",
        "rule" => "startup",
        "other-agent" => "never",
        _ => "on-demand",
    }
    .to_string();

    let paths = if kind == "rule" { rule_paths(text) } else { Vec::new() };
    if !paths.is_empty() {
        loads = "on-demand".into();
    }
    let imports = if matches!(kind, "claude" | "local" | "rule" | "nested") {
        imports_of(path, text, ctx.scope, ctx.home)
    } else {
        Vec::new()
    };
    for i in imports.iter().filter(|i| !i.exists) {
        warnings.push(format!("@{} doesn't exist — the import is ignored", i.raw));
    }
    if imports.iter().any(|i| i.exists && i.external) {
        warnings.push("imports files outside the project — Claude Code asks once before loading them".into());
    }

    if content.text.is_none() && content.bytes > MAX_FILE {
        warnings.push("over 4 MiB — Claude Code skips this file".into());
        loads = "never".into();
    } else if matches!(kind, "claude" | "local" | "nested") && content.lines > RECOMMENDED_LINES {
        warnings.push(format!(
            "{} lines — the docs recommend under {RECOMMENDED_LINES} for good adherence",
            content.lines
        ));
    }

    let excluded = matches!(kind, "claude" | "local" | "rule" | "nested") && ctx.excludes.matches(path);
    if excluded {
        loads = "never".into();
    }

    InstrFile {
        path: s(path),
        label,
        kind: kind.into(),
        loads,
        editable,
        bytes: content.bytes,
        lines: content.lines,
        excluded,
        paths,
        agent: None,
        imported_by: Vec::new(),
        applies_to: Vec::new(),
        imports,
        warnings,
    }
}

pub(crate) fn md_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out: Vec<PathBuf> = WalkDir::new(dir)
        .follow_links(true)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false))
        .collect();
    out.sort();
    out
}

pub(crate) fn rel_label(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| s(path))
}

/// Rules under a `rules` dir: files and their labels.
fn rule_files(rules_dir: &Path, root: &Path, ctx: &Ctx, editable: bool) -> Vec<InstrFile> {
    md_files(rules_dir)
        .iter()
        .map(|p| build_file(p, rel_label(p, root), "rule", editable, ctx))
        .collect()
}

fn other_agent_files(root: &Path, specs: &[(&str, &str)], dirs: &[(&str, &str)], ctx: &Ctx) -> Vec<InstrFile> {
    let mut out = Vec::new();
    for (rel, agent) in specs {
        let p = root.join(rel);
        if p.is_file() {
            let mut f = build_file(&p, rel.to_string(), "other-agent", true, ctx);
            f.agent = Some(agent.to_string());
            out.push(f);
        }
    }
    for (rel, agent) in dirs {
        let d = root.join(rel);
        if !d.is_dir() {
            continue;
        }
        for p in WalkDir::new(&d).max_depth(4).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
            let mut f = build_file(p.path(), rel_label(p.path(), root), "other-agent", true, ctx);
            f.agent = Some(agent.to_string());
            out.push(f);
        }
    }
    out
}

/// Subdirectory CLAUDE.md files and nested `.claude/rules` (both on demand).
/// Returns (files, scan was cut short).
fn nested_scan(project: &Path) -> (Vec<PathBuf>, bool) {
    let mut found = Vec::new();
    let mut seen = 0usize;
    let walker = ignore::WalkBuilder::new(project)
        .hidden(false)
        .git_global(false)
        .max_depth(Some(NESTED_DEPTH))
        .follow_links(false)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            // The project's own .claude is handled directly.
            !(is_dir && (SKIP_DIRS.contains(&name.as_ref()) || (e.depth() == 1 && name == ".claude")))
        })
        .build();
    for entry in walker {
        seen += 1;
        if seen > NESTED_BUDGET {
            return (found, true);
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy();
        // Judge by the path inside the project: the project itself may live
        // under a `.claude` folder.
        let rel: Vec<String> = path
            .strip_prefix(project)
            .map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        let in_rules = rel.windows(2).any(|w| w[0] == ".claude" && w[1] == "rules");
        if (entry.depth() >= 2 && CLAUDE_NAMES.contains(&name.as_ref()))
            || (in_rules && name.to_lowercase().ends_with(".md"))
        {
            found.push(path.to_path_buf());
        }
    }
    found.sort();
    (found, false)
}

type Scan = (Vec<PathBuf>, bool);

/// Nested scans walk whole repos; reuse recent results unless forced.
const SCAN_TTL: std::time::Duration = std::time::Duration::from_secs(120);

fn scan_cache() -> &'static Mutex<HashMap<String, (std::time::Instant, Scan)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (std::time::Instant, Scan)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn nested_scans(todo: &[&PathBuf], force: bool) -> HashMap<String, Scan> {
    let mut out = HashMap::new();
    let mut missing: Vec<&PathBuf> = Vec::new();
    {
        let cache = scan_cache().lock().unwrap();
        for p in todo {
            match cache.get(&path_key(p)) {
                Some((at, scan)) if !force && at.elapsed() < SCAN_TTL => {
                    out.insert(path_key(p), scan.clone());
                }
                _ => missing.push(p),
            }
        }
    }
    let chunk = missing.len().div_ceil(4).max(1);
    let fresh: Vec<(String, Scan)> = std::thread::scope(|sc| {
        let handles: Vec<_> = missing
            .chunks(chunk)
            .map(|batch| {
                sc.spawn(move || batch.iter().map(|p| (path_key(p), nested_scan(p))).collect::<Vec<_>>())
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    let now = std::time::Instant::now();
    let mut cache = scan_cache().lock().unwrap();
    for (k, scan) in fresh {
        cache.insert(k.clone(), (now, scan.clone()));
        out.insert(k, scan);
    }
    out
}

fn skip_nested(project: &Path, home: &Path) -> bool {
    path_key(project) == path_key(home) || project.parent().is_none() || project.parent() == Some(Path::new(""))
}

/// Registered projects that exist, deduplicated.
pub(crate) fn projects() -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out: Vec<PathBuf> = claude_project_paths()
        .into_iter()
        .filter(|p| p.is_dir())
        .filter(|p| seen.insert(path_key(&canon(p))))
        .collect();
    out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    out
}

pub(crate) fn project_label(p: &Path) -> String {
    if home_dir().map(|h| path_key(&h) == path_key(p)).unwrap_or(false) {
        return "Home folder (~)".into();
    }
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| s(p))
}

// ---------------------------------------------------------------------------
// Startup context
// ---------------------------------------------------------------------------

fn push_startup(out: &mut Vec<StartupEntry>, file: &Path, label: String, depth: usize, home: &Path, visited: &mut HashSet<String>) {
    if !visited.insert(path_key(file)) {
        return;
    }
    let content = read_content(file);
    let is_memory = file.file_name().map(|n| n == "MEMORY.md").unwrap_or(false);
    let text = content.text.clone().unwrap_or_default();
    let (loaded, note) = if content.text.is_none() {
        (0, Some("skipped: over 4 MiB".to_string()))
    } else if is_memory {
        let l = memory_loaded_bytes(&text);
        (l, (l < content.bytes).then(|| "truncated to 200 lines / 25 KB".to_string()))
    } else {
        (content.bytes, None)
    };
    out.push(StartupEntry {
        path: s(file),
        label,
        depth,
        bytes: content.bytes,
        loaded_bytes: loaded,
        lines: content.lines,
        note,
    });
    if depth >= IMPORT_DEPTH || is_memory {
        return;
    }
    for raw in parse_imports(&text) {
        let target = resolve_import(&raw, file, home);
        if target.is_file() {
            push_startup(out, &target, format!("@{raw}"), depth + 1, home, visited);
        }
    }
}

fn managed_inline() -> Vec<(String, String)> {
    managed_settings_files()
        .into_iter()
        .filter_map(|f| {
            let text = settings_value(&f, "claudeMd")?.as_str()?.to_string();
            Some((f.file_name()?.to_string_lossy().to_string(), text))
        })
        .collect()
}

fn managed_startup(out: &mut Vec<StartupEntry>, home: &Path, visited: &mut HashSet<String>) {
    let file = managed_dir().join("CLAUDE.md");
    if file.is_file() {
        push_startup(out, &file, "Managed CLAUDE.md".into(), 0, home, visited);
    }
    for (name, text) in managed_inline() {
        out.push(StartupEntry {
            path: String::new(),
            label: format!("Managed claudeMd ({name})"),
            depth: 0,
            bytes: text.len() as u64,
            loaded_bytes: text.len() as u64,
            lines: text.lines().count(),
            note: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

/// `force` rescans nested CLAUDE.md files instead of using recent results.
pub fn overview(force: bool) -> Result<InstrOverview, String> {
    let home = home_dir()?;
    let claude = claude_dir()?;
    let user_scope = scope_files(&claude);
    let managed_files = managed_settings_files();
    let managed_ex = exclude_patterns(&managed_files);
    let user_ex = exclude_patterns(&user_scope);
    let user_excludes = Excludes::new(&[managed_ex.clone(), user_ex.clone()].concat());
    let projects = projects();

    let mut groups: Vec<InstrGroup> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let claim = |files: Vec<InstrFile>, seen: &mut HashSet<String>| -> Vec<InstrFile> {
        files.into_iter().filter(|f| seen.insert(path_key(Path::new(&f.path)))).collect()
    };
    let managed_layers: Vec<(PathBuf, &str)> =
        managed_files.iter().map(|f| (f.clone(), "managed policy")).collect();

    // Managed policy.
    let mdir = managed_dir();
    let managed_md = mdir.join("CLAUDE.md");
    let none = Excludes(None);
    let mctx = Ctx { home: &home, scope: None, excludes: &none };
    let mut mfiles = Vec::new();
    if managed_md.is_file() {
        mfiles.push(build_file(&managed_md, "CLAUDE.md".into(), "claude", false, &mctx));
    }
    let inline = managed_inline();
    if !mfiles.is_empty() || !inline.is_empty() {
        let mut startup = Vec::new();
        managed_startup(&mut startup, &home, &mut HashSet::new());
        groups.push(InstrGroup {
            key: "managed".into(),
            kind: "managed".into(),
            label: "Managed policy".into(),
            detail: format!("{} (read-only; can't be excluded)", s(&mdir)),
            project_dir: None,
            files: claim(mfiles, &mut seen),
            startup,
            auto_memory: None,
            notes: inline
                .iter()
                .map(|(name, text)| format!("{name} sets claudeMd ({} lines)", text.lines().count()))
                .collect(),
        });
    }

    // User.
    let uctx = Ctx { home: &home, scope: None, excludes: &user_excludes };
    let mut ufiles = Vec::new();
    let user_md = claude.join("CLAUDE.md");
    if user_md.is_file() {
        ufiles.push(build_file(&user_md, "CLAUDE.md".into(), "claude", true, &uctx));
    }
    ufiles.extend(rule_files(&claude.join("rules"), &claude, &uctx, true));
    let mut user_layers = managed_layers.clone();
    user_layers.push((user_scope[1].clone(), "user settings.local.json"));
    user_layers.push((user_scope[0].clone(), "user settings.json"));
    // Auto-memory *state* stays here (the Config tab shows it in the startup
    // context); the notes themselves belong to the Memory tab.
    let user_mem = auto_memory_state(&user_layers, claude.join("projects"), &home);
    ufiles.extend(other_agent_files(&home, &USER_AGENT_FILES, &[], &uctx));
    let mut user_startup = Vec::new();
    let mut visited = HashSet::new();
    managed_startup(&mut user_startup, &home, &mut visited);
    if user_md.is_file() && !user_excludes.matches(&user_md) {
        push_startup(&mut user_startup, &user_md, "~/.claude/CLAUDE.md".into(), 0, &home, &mut visited);
    }
    for r in ufiles.iter().filter(|f| f.kind == "rule" && f.loads == "startup") {
        push_startup(&mut user_startup, Path::new(&r.path), format!("~/.claude/{}", r.label), 0, &home, &mut visited);
    }
    let user_startup_prefix = user_startup.clone();
    groups.push(InstrGroup {
        key: "user".into(),
        kind: "user".into(),
        label: "User instructions".into(),
        detail: s(&claude),
        project_dir: None,
        files: claim(ufiles, &mut seen),
        startup: user_startup,
        auto_memory: Some(user_mem),
        notes: Vec::new(),
    });

    // Nested scans run in parallel (and are cached) — they're the slow part.
    let todo: Vec<&PathBuf> = projects.iter().filter(|p| !skip_nested(p, &home)).collect();
    let scans = nested_scans(&todo, force);

    let project_keys: HashSet<String> = projects.iter().map(|p| path_key(p)).collect();
    let mut ancestors: BTreeMap<String, (PathBuf, Vec<String>)> = BTreeMap::new();

    for project in &projects {
        let pclaude = project.join(".claude");
        let pscope = scope_files(&pclaude);
        let excludes = Excludes::new(
            &[managed_ex.clone(), user_ex.clone(), exclude_patterns(&pscope)].concat(),
        );
        let ctx = Ctx { home: &home, scope: Some(project), excludes: &excludes };
        let mut files = Vec::new();
        let mut notes = Vec::new();

        for (rel, kind) in [("CLAUDE.md", "claude"), (".claude/CLAUDE.md", "claude"), ("CLAUDE.local.md", "local")] {
            let p = project.join(rel);
            if p.is_file() {
                files.push(build_file(&p, rel.into(), kind, true, &ctx));
            }
        }
        files.extend(rule_files(&pclaude.join("rules"), project, &ctx, true));
        match scans.get(&path_key(project)) {
            Some((nested, truncated)) => {
                for p in nested {
                    let kind = if CLAUDE_NAMES.iter().any(|n| p.file_name().map(|f| f == *n).unwrap_or(false)) {
                        "nested"
                    } else {
                        "rule"
                    };
                    let mut f = build_file(p, rel_label(p, project), kind, true, &ctx);
                    if f.loads == "startup" {
                        f.loads = "on-demand".into();
                    }
                    files.push(f);
                }
                if *truncated {
                    notes.push(format!(
                        "Stopped looking for nested CLAUDE.md files after {NESTED_BUDGET} entries — some may be missing."
                    ));
                }
            }
            None => notes.push("Nested CLAUDE.md files aren't scanned for this folder (it's your home or a drive root).".into()),
        }

        let mut layers = managed_layers.clone();
        layers.push((pscope[1].clone(), "project settings.local.json"));
        layers.push((pscope[0].clone(), "project settings.json"));
        layers.extend(user_layers.iter().skip(managed_layers.len()).cloned());
        let mem = auto_memory_state(
            &layers,
            claude.join("projects").join(encode_project(&git_root(project))).join("memory"),
            &home,
        );
        files.extend(other_agent_files(project, &OTHER_AGENT_FILES, &OTHER_AGENT_DIRS, &ctx));

        // Startup context: what every session in this project gets.
        let mut startup = user_startup_prefix.clone();
        let mut visited: HashSet<String> = startup.iter().map(|e| path_key(Path::new(&e.path))).collect();
        let mut chain: Vec<&Path> = project.ancestors().skip(1).collect();
        chain.reverse();
        for dir in chain {
            for name in CLAUDE_NAMES {
                let p = dir.join(name);
                if !p.is_file() {
                    continue;
                }
                if excludes.matches(&p) {
                    continue;
                }
                push_startup(&mut startup, &p, s(&p), 0, &home, &mut visited);
                if !project_keys.contains(&path_key(dir)) {
                    let e = ancestors.entry(path_key(&p)).or_insert_with(|| (p.clone(), Vec::new()));
                    e.1.push(project_label(project));
                }
            }
        }
        for rel in ["CLAUDE.md", ".claude/CLAUDE.md"] {
            let p = project.join(rel);
            if p.is_file() && !excludes.matches(&p) {
                push_startup(&mut startup, &p, rel.into(), 0, &home, &mut visited);
            }
        }
        for r in files.iter().filter(|f| f.kind == "rule" && f.loads == "startup") {
            push_startup(&mut startup, Path::new(&r.path), r.label.clone(), 0, &home, &mut visited);
        }
        let local = project.join("CLAUDE.local.md");
        if local.is_file() && !excludes.matches(&local) {
            push_startup(&mut startup, &local, "CLAUDE.local.md".into(), 0, &home, &mut visited);
        }
        let index = Path::new(&mem.dir).join("MEMORY.md");
        if mem.enabled && index.is_file() {
            push_startup(&mut startup, &index, "auto memory MEMORY.md".into(), 0, &home, &mut visited);
        }

        // Other agents' files Claude reads anyway because something imports them.
        let importers: Vec<(String, String)> = files
            .iter()
            .flat_map(|f| f.imports.iter().map(move |i| (path_key(Path::new(&i.path)), f.label.clone())))
            .collect();
        for f in files.iter_mut().filter(|f| f.kind == "other-agent") {
            let key = path_key(Path::new(&f.path));
            f.imported_by = importers.iter().filter(|(k, _)| *k == key).map(|(_, l)| l.clone()).collect();
            if !f.imported_by.is_empty() {
                f.loads = "imported".into();
            }
        }

        let files = claim(files, &mut seen);
        if files.is_empty() {
            continue;
        }
        groups.push(InstrGroup {
            key: format!("project:{}", project.display()),
            kind: "project".into(),
            label: project_label(project),
            detail: s(project),
            project_dir: Some(s(project)),
            files,
            startup,
            auto_memory: Some(mem),
            notes,
        });
    }

    // Ancestor folders that aren't projects themselves.
    let mut parent_files = Vec::new();
    for (_, (p, applies)) in ancestors {
        let mut f = build_file(&p, s(&p), if p.file_name().map(|n| n == "CLAUDE.local.md").unwrap_or(false) { "local" } else { "claude" }, true, &uctx);
        f.applies_to = applies;
        parent_files.push(f);
    }
    let parent_files = claim(parent_files, &mut seen);
    if !parent_files.is_empty() {
        groups.push(InstrGroup {
            key: "parents".into(),
            kind: "parents".into(),
            label: "Parent folders".into(),
            detail: "CLAUDE.md files above your projects — loaded for every project below them".into(),
            project_dir: None,
            files: parent_files,
            startup: Vec::new(),
            auto_memory: None,
            notes: Vec::new(),
        });
    }

    // Remember resolved imports so the file guard can admit them.
    let imports: HashSet<String> = groups
        .iter()
        .flat_map(|g| g.files.iter().flat_map(|f| f.imports.iter()))
        .filter(|i| i.exists)
        .map(|i| path_key(&canon(Path::new(&i.path))))
        .collect();
    *import_cache().lock().unwrap() = imports;

    Ok(InstrOverview {
        groups,
        projects: projects.iter().map(|p| ProjectRef { dir: s(p), label: project_label(p) }).collect(),
    })
}

// ---------------------------------------------------------------------------
// File guard
// ---------------------------------------------------------------------------

fn is_instruction_name(name: &str) -> bool {
    CLAUDE_NAMES.contains(&name)
}

/// Whether the file editor may open (or, with `write`, change) `path`.
pub fn is_allowed(path: &Path, write: bool) -> bool {
    let Ok(p) = path.canonicalize().map(|c| strip_verbatim(&c)) else { return false };
    let Ok(home) = home_dir().map(|h| canon(&h)) else { return false };
    let Ok(claude) = claude_dir().map(|c| canon(&c)) else { return false };
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let is_md = name.to_lowercase().ends_with(".md");

    if under(&p, &managed_dir()) {
        return !write && path_key(&p) == path_key(&managed_dir().join("CLAUDE.md"));
    }
    if import_cache().lock().unwrap().contains(&path_key(&p)) {
        return true;
    }
    if path_key(&p) == path_key(&claude.join("CLAUDE.md")) {
        return true;
    }
    if is_md && under(&p, &claude.join("rules")) {
        return true;
    }
    if is_md && under(&p, &claude.join("projects")) && p.components().any(|c| c.as_os_str() == "memory") {
        return true;
    }
    if USER_AGENT_FILES.iter().any(|(rel, _)| path_key(&p) == path_key(&home.join(rel))) {
        return true;
    }
    let user_layers = scope_files(&claude);
    if let Some(dir) = user_layers.iter().find_map(|f| settings_value(f, "autoMemoryDirectory").and_then(|v| v.as_str().map(str::to_string))) {
        if is_md && under(&p, &expand_home(&dir, &home)) {
            return true;
        }
    }
    for project in projects() {
        let project = canon(&project);
        if under(&p, &project) {
            if is_instruction_name(&name) {
                return true;
            }
            let rel = rel_label(&p, &project);
            if OTHER_AGENT_FILES.iter().any(|(r, _)| rel.eq_ignore_ascii_case(r)) {
                return true;
            }
            if OTHER_AGENT_DIRS.iter().any(|(d, _)| under(&p, &project.join(d))) {
                return true;
            }
            if is_md && p.components().any(|c| c.as_os_str() == ".claude") && p.components().any(|c| c.as_os_str() == "rules") {
                return true;
            }
        }
        // CLAUDE.md files in folders above a project.
        if is_instruction_name(&name) && p.parent().map(|d| under(&project, d)).unwrap_or(false) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

fn registered_project(dir: &str) -> Result<PathBuf, String> {
    let want = path_key(&canon(Path::new(dir)));
    projects()
        .into_iter()
        .find(|p| path_key(&canon(p)) == want)
        .ok_or_else(|| format!("{dir} is not a registered Claude Code project"))
}

fn template(kind: &str, title: &str, paths: &[String]) -> String {
    match kind {
        "local" => "# Personal notes for this project\n\nNot committed — only you see these.\n".into(),
        "rule" => {
            let mut t = String::new();
            let paths: Vec<&String> = paths.iter().filter(|p| !p.trim().is_empty()).collect();
            if !paths.is_empty() {
                t.push_str("---\npaths:\n");
                for p in paths {
                    t.push_str(&format!("  - \"{}\"\n", p.trim().replace('"', "\\\"")));
                }
                t.push_str("---\n\n");
            }
            t.push_str(&format!("# {title}\n\n"));
            t
        }
        "agents" => format!("# {title}\n\nInstructions for coding agents working in this repository.\n"),
        _ => format!("# {title}\n\n"),
    }
}

/// Add `CLAUDE.local.md` to the project's .gitignore unless it's covered.
fn gitignore_local(project: &Path) -> Result<Option<String>, String> {
    if !git_root(project).join(".git").exists() {
        return Ok(None);
    }
    let file = project.join(".gitignore");
    let existing = fs::read_to_string(&file).unwrap_or_default();
    if existing.lines().any(|l| matches!(l.trim(), "CLAUDE.local.md" | "/CLAUDE.local.md" | "*.local.md")) {
        return Ok(None);
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("CLAUDE.local.md\n");
    fs::write(&file, text).map_err(|e| format!("cannot update {}: {e}", file.display()))?;
    Ok(Some("added CLAUDE.local.md to .gitignore".into()))
}

/// Create a standard instruction file. `project` = None means user level.
/// kinds: "claude", "dot-claude", "local", "rule", "agents", "gemini".
pub fn create(
    project: Option<&str>,
    kind: &str,
    name: Option<&str>,
    paths: &[String],
    gitignore: bool,
) -> Result<(String, Option<String>), String> {
    let claude = claude_dir()?;
    let (root, title) = match project {
        None => (None, "Personal instructions".to_string()),
        Some(dir) => {
            let p = registered_project(dir)?;
            let t = project_label(&p);
            (Some(p), t)
        }
    };
    let rule_name = || -> Result<String, String> {
        let n = name.map(str::trim).unwrap_or("");
        if n.is_empty() || n.contains(['/', '\\', ':']) || n.starts_with('.') {
            return Err("rule name must be a plain file name".into());
        }
        Ok(if n.to_lowercase().ends_with(".md") { n.to_string() } else { format!("{n}.md") })
    };
    let target = match (&root, kind) {
        (None, "claude") => claude.join("CLAUDE.md"),
        (None, "rule") => claude.join("rules").join(rule_name()?),
        (Some(p), "claude") => p.join("CLAUDE.md"),
        (Some(p), "dot-claude") => p.join(".claude").join("CLAUDE.md"),
        (Some(p), "local") => p.join("CLAUDE.local.md"),
        (Some(p), "rule") => p.join(".claude").join("rules").join(rule_name()?),
        (Some(p), "agents") => p.join("AGENTS.md"),
        (Some(p), "gemini") => p.join("GEMINI.md"),
        _ => return Err(format!("can't create a {kind} file there")),
    };
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    let heading = if kind == "rule" {
        target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        title
    };
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::write(&target, template(kind, &heading, paths))
        .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let note = match (&root, kind, gitignore) {
        (Some(p), "local", true) => gitignore_local(p)?,
        _ => None,
    };
    Ok((s(&target), note))
}

pub fn delete(path: &str) -> Result<(), String> {
    let p = PathBuf::from(path);
    if !p.is_file() {
        return Err(format!("{path} is not a file"));
    }
    if !is_allowed(&p, true) {
        return Err(format!("{path} isn't an instruction file this app manages"));
    }
    fs::remove_file(&p).map_err(|e| format!("cannot delete {path}: {e}"))
}

/// Turn auto memory on or off for the user, or for one project (written to
/// its `.claude/settings.local.json`, which overrides shared settings).
pub fn set_auto_memory(project: Option<&str>, enabled: bool) -> Result<String, String> {
    let claude = claude_dir()?;
    let file = match project {
        None => claude.join("settings.json"),
        Some(dir) => registered_project(dir)?.join(".claude").join("settings.local.json"),
    };
    let (mut obj, _) = load_object(&file)?;
    if project.is_none() && enabled {
        obj.shift_remove("autoMemoryEnabled");
    } else {
        obj.insert("autoMemoryEnabled".into(), Value::Bool(enabled));
    }
    write_object(&file, &obj)?;
    let mut msg = format!(
        "Auto memory {} ({})",
        if enabled { "on" } else { "off" },
        file.display()
    );
    if env_disables_auto_memory() {
        msg.push_str(" — note: CLAUDE_CODE_DISABLE_AUTO_MEMORY is set and still turns it off");
    }
    Ok(msg)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "skills-editor-instr-{tag}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    /// Prints what the scan finds on the real machine and how long it takes:
    /// `cargo test live_instructions -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_instructions() {
        let started = std::time::Instant::now();
        let ov = overview(true).unwrap();
        println!("scan took {:?}; {} projects", started.elapsed(), ov.projects.len());
        for g in &ov.groups {
            println!("[{}] {} — {} files, {} startup entries", g.kind, g.label, g.files.len(), g.startup.len());
            for n in &g.notes {
                println!("   note: {n}");
            }
            if let Some(m) = &g.auto_memory {
                println!("   auto memory: enabled={} ({}) exists={} {}", m.enabled, m.source, m.exists, m.dir);
            }
            for f in g.files.iter().take(12) {
                println!(
                    "   {:<45} {:<13} {:<9} {:>6}B imports={} warn={:?}",
                    f.label, f.kind, f.loads, f.bytes, f.imports.len(), f.warnings
                );
            }
            if g.files.len() > 12 {
                println!("   … {} more", g.files.len() - 12);
            }
            let total: u64 = g.startup.iter().map(|e| e.loaded_bytes).sum();
            if !g.startup.is_empty() {
                println!("   startup ≈ {} tokens: {:?}", total / 4, g.startup.iter().map(|e| e.label.as_str()).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn encodes_project_dirs_like_claude_code() {
        assert_eq!(
            encode_project(Path::new(r"C:\Users\me\.t3\worktrees\X")),
            "C--Users-me--t3-worktrees-X"
        );
        assert_eq!(encode_project(Path::new("/home/me/my_repo")), "-home-me-my-repo");
    }

    #[test]
    fn parses_imports_but_not_code_or_emails() {
        let text = "See @README and @docs/git.md.\n\
            Mail me@example.com, or `@not/this.md`.\n\
            ```\n@inside/fence.md\n```\n\
            - @~/.claude/extra.md\n\
            (see @AGENTS.md)\n\
            @README again\n";
        assert_eq!(
            parse_imports(text),
            vec!["README", "docs/git.md", "~/.claude/extra.md", "AGENTS.md"]
        );
    }

    #[test]
    fn import_resolution_and_path_heuristics() {
        let home = Path::new("/h");
        let from = Path::new("/proj/sub/CLAUDE.md");
        assert_eq!(resolve_import("../x.md", from, home), normalize(Path::new("/proj/x.md")));
        assert_eq!(resolve_import("~/a.md", from, home), normalize(Path::new("/h/a.md")));
        assert!(looks_like_path("docs/a.md"));
        assert!(looks_like_path("~/x"));
        assert!(!looks_like_path("anthropic-ai/claude-code"));
        assert!(!looks_like_path("someone"));
    }

    #[test]
    fn memory_truncation_limits() {
        let short = "a\n".repeat(10);
        assert_eq!(memory_loaded_bytes(&short), short.len() as u64);
        let long = "a\n".repeat(300);
        assert_eq!(memory_loaded_bytes(&long), 400);
        let wide = format!("{}\n", "x".repeat(30 * 1024));
        assert_eq!(memory_loaded_bytes(&wide), 0);
    }

    #[test]
    fn excludes_match_absolute_forward_slash_paths() {
        let ex = Excludes::new(&["**/other-team/CLAUDE.md".into()]);
        assert!(ex.matches(Path::new("/repo/other-team/CLAUDE.md")));
        assert!(!ex.matches(Path::new("/repo/mine/CLAUDE.md")));
        assert!(!Excludes::new(&[]).matches(Path::new("/x/CLAUDE.md")));
    }

    #[test]
    fn nested_scan_respects_gitignore_and_skips() {
        let root = scratch("nested");
        fs::create_dir_all(root.join(".git")).unwrap();
        write(&root.join(".gitignore"), "ignored/\n");
        write(&root.join("CLAUDE.md"), "top");
        write(&root.join("pkg/CLAUDE.md"), "nested");
        write(&root.join("pkg/CLAUDE.local.md"), "nested local");
        write(&root.join("pkg/.claude/rules/a.md"), "rule");
        write(&root.join("ignored/CLAUDE.md"), "no");
        write(&root.join("node_modules/x/CLAUDE.md"), "no");
        write(&root.join(".claude/rules/top.md"), "handled elsewhere");
        let (found, truncated) = nested_scan(&root);
        let rels: Vec<String> = found.iter().map(|p| rel_label(p, &root)).collect();
        assert_eq!(rels, vec!["pkg/.claude/rules/a.md", "pkg/CLAUDE.local.md", "pkg/CLAUDE.md"]);
        assert!(!truncated);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn end_to_end_overview_create_toggle_and_guard() {
        let home = scratch("home");
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let claude = home.join(".claude");
        let proj = home.join("work").join("app");
        fs::create_dir_all(proj.join(".git")).unwrap();

        write(&claude.join("CLAUDE.md"), "# Me\n@~/shared.md\n");
        write(&home.join("shared.md"), "shared\n");
        write(&claude.join("rules/style.md"), "be terse\n");
        write(&home.join("work/CLAUDE.md"), "parent folder rules\n");
        write(&proj.join("CLAUDE.md"), "# App\n@AGENTS.md\n@docs/missing.md\n");
        write(&proj.join("AGENTS.md"), "agents\n");
        write(&proj.join("GEMINI.md"), "gemini\n");
        write(&proj.join("CLAUDE.local.md"), "mine\n");
        write(&proj.join(".claude/rules/api.md"), "---\npaths:\n  - \"src/api/**\"\n---\napi\n");
        write(&proj.join(".claude/rules/always.md"), "always\n");
        write(&proj.join("pkg/CLAUDE.md"), "nested\n");
        write(&proj.join("vendor/CLAUDE.md"), "excluded\n");
        write(&proj.join(".claude/settings.local.json"), r#"{"claudeMdExcludes":["**/vendor/CLAUDE.md"]}"#);
        let mem = claude.join("projects").join(encode_project(&proj)).join("memory");
        write(&mem.join("MEMORY.md"), &"- line\n".repeat(250));
        write(&mem.join("user_role.md"), "---\nname: role\ndescription: who I am\ntype: user\n---\nbody\n");
        let stray = claude.join("projects").join("C--old-worktree").join("memory");
        write(&stray.join("MEMORY.md"), "old\n");
        write(
            &home.join(".claude.json"),
            &serde_json::json!({ "projects": {
                (fwd(&proj)): {},
                (fwd(&home)): {},
            }})
            .to_string(),
        );

        let ov = overview(true).unwrap();
        let group = |kind: &str| ov.groups.iter().find(|g| g.kind == kind).unwrap_or_else(|| panic!("no {kind} group"));
        let file = |g: &InstrGroup, label: &str| g.files.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no {label}")).clone();

        let user = group("user");
        assert_eq!(file(user, "CLAUDE.md").imports[0].raw, "~/shared.md");
        assert_eq!(file(user, "rules/style.md").loads, "startup");

        let app = ov.groups.iter().find(|g| g.label == "app").expect("project group");
        let claude_md = file(app, "CLAUDE.md");
        assert_eq!(claude_md.imports.len(), 2);
        assert!(claude_md.warnings.iter().any(|w| w.contains("docs/missing.md")));
        let agents = file(app, "AGENTS.md");
        assert_eq!(agents.loads, "imported");
        assert_eq!(agents.imported_by, vec!["CLAUDE.md"]);
        assert_eq!(file(app, "GEMINI.md").loads, "never");
        assert_eq!(file(app, ".claude/rules/api.md").loads, "on-demand");
        assert_eq!(file(app, ".claude/rules/api.md").paths, vec!["src/api/**"]);
        assert_eq!(file(app, "pkg/CLAUDE.md").loads, "on-demand");
        let vendor = file(app, "vendor/CLAUDE.md");
        assert!(vendor.excluded);
        assert_eq!(vendor.loads, "never");
        assert!(app.files.iter().all(|f| !f.path.contains("projects")), "memory notes belong to the Memory tab");

        // Startup order: user, its import, user rules, parent folder, project, rules, local, memory.
        let labels: Vec<&str> = app.startup.iter().map(|e| e.label.as_str()).collect();
        let pos = |needle: &str| labels.iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} not in {labels:?}"));
        assert!(pos("~/.claude/CLAUDE.md") < pos("@~/shared.md"));
        assert!(pos("@~/shared.md") < pos("rules/style.md"));
        assert!(pos("rules/style.md") < pos("work"));
        assert!(pos("work") < pos("@AGENTS.md"));
        assert!(pos("@AGENTS.md") < pos("rules/always.md"));
        assert!(pos("rules/always.md") < pos("CLAUDE.local.md"));
        assert!(pos("CLAUDE.local.md") < pos("MEMORY.md"));
        assert!(!labels.iter().any(|l| l.contains("api.md") || l.contains("pkg") || l.contains("vendor")));
        let memory_entry = &app.startup[pos("MEMORY.md")];
        assert_eq!(memory_entry.loaded_bytes, 200 * 7);

        // The parent-folder file is listed once, with the projects it applies to.
        let parents = group("parents");
        assert_eq!(parents.files.len(), 1);
        assert_eq!(parents.files[0].applies_to, vec!["app"]);
        // The home "project" adds nothing new (its files are the user's).
        assert!(ov.groups.iter().all(|g| g.label != "Home folder (~)"));
        assert!(ov.projects.iter().any(|p| p.label == "Home folder (~)"));

        // Guard.
        assert!(is_allowed(&proj.join("AGENTS.md"), true));
        assert!(is_allowed(&home.join("shared.md"), true), "imported file admitted via cache");
        assert!(is_allowed(&home.join("work/CLAUDE.md"), true), "ancestor CLAUDE.md");
        assert!(is_allowed(&topic_path(&mem), true));
        write(&proj.join("src/main.rs"), "fn main() {}");
        assert!(!is_allowed(&proj.join("src/main.rs"), false));

        // Create: local file + .gitignore entry, refuse duplicates and bad names.
        write(&proj.join(".gitignore"), "target");
        let _ = fs::remove_file(proj.join("CLAUDE.local.md"));
        let (path, note) = create(Some(&s(&proj)), "local", None, &[], true).unwrap();
        assert!(Path::new(&path).is_file());
        assert!(note.is_some());
        assert_eq!(fs::read_to_string(proj.join(".gitignore")).unwrap(), "target\nCLAUDE.local.md\n");
        assert!(create(Some(&s(&proj)), "local", None, &[], true).is_err());
        let (rule, _) = create(Some(&s(&proj)), "rule", Some("tests"), &["**/*.test.ts".into()], false).unwrap();
        assert!(fs::read_to_string(&rule).unwrap().starts_with("---\npaths:\n  - \"**/*.test.ts\"\n---"));
        assert!(create(Some(&s(&proj)), "rule", Some("../evil"), &[], false).is_err());
        assert!(create(Some(&s(&home.join("nope"))), "claude", None, &[], false).is_err());
        delete(&rule).unwrap();
        assert!(delete(&s(&proj.join("src/main.rs"))).is_err());

        // Auto memory toggle.
        set_auto_memory(Some(&s(&proj)), false).unwrap();
        let local: Value = serde_json::from_str(&fs::read_to_string(proj.join(".claude/settings.local.json")).unwrap()).unwrap();
        assert_eq!(local["autoMemoryEnabled"], false);
        assert_eq!(local["claudeMdExcludes"][0], "**/vendor/CLAUDE.md", "other keys kept");
        let ov = overview(true).unwrap();
        let app = ov.groups.iter().find(|g| g.label == "app").unwrap();
        let am = app.auto_memory.as_ref().unwrap();
        assert!(!am.enabled);
        assert_eq!(am.source, "project settings.local.json");
        assert!(!app.startup.iter().any(|e| e.label.contains("MEMORY.md")));
        set_auto_memory(None, false).unwrap();
        set_auto_memory(None, true).unwrap();
        assert!(!fs::read_to_string(claude.join("settings.json")).unwrap().contains("autoMemoryEnabled"));

        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = fs::remove_dir_all(home);
    }

    fn topic_path(mem: &Path) -> PathBuf {
        mem.join("user_role.md")
    }
}
