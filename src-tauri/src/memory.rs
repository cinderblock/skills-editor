//! Auto memory: the notes Claude writes for itself, per project and per
//! subagent. Distinct from the instruction files in `instructions.rs`, which
//! are configuration you write. See plans/memory.md.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use walkdir::WalkDir;

use crate::instructions::{
    auto_memory_state, canon, encode_project, frontmatter, git_root, md_files, memory_loaded_bytes,
    project_label, projects, read_content, rel_label, s, scope_files, under, yaml_str,
    MEMORY_MAX_LINES,
};
use crate::paths::{claude_dir, home_dir, path_key, strip_verbatim};

/// Subagent memory scopes: (frontmatter value, dir relative to its root).
const AGENT_SCOPES: [(&str, &str); 3] = [
    ("user", "agent-memory"),
    ("project", ".claude/agent-memory"),
    ("local", ".claude/agent-memory-local"),
];

pub const MEMORY_TYPES: [&str; 4] = ["user", "feedback", "project", "reference"];

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct MemoryEntry {
    pub path: String,
    /// Path relative to the store (usually just the file name).
    pub rel: String,
    /// Frontmatter `name`, else the file stem.
    pub name: String,
    pub description: Option<String>,
    /// user | feedback | project | reference (top-level or metadata.type).
    pub kind: Option<String>,
    /// Frontmatter `modified`, when Claude Code recorded one.
    pub modified: Option<String>,
    /// File mtime, epoch seconds — a fallback for age.
    pub mtime: u64,
    pub bytes: u64,
    pub lines: usize,
    pub in_index: bool,
    pub has_frontmatter: bool,
}

#[derive(Serialize, Clone)]
pub struct IndexLink {
    pub title: String,
    pub target: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Serialize, Clone)]
pub struct MemoryIndex {
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
    pub lines: usize,
    /// What actually loads at session start.
    pub loaded_bytes: u64,
    pub truncated: bool,
    pub links: Vec<IndexLink>,
}

#[derive(Serialize, Clone)]
pub struct AgentInfo {
    pub name: String,
    /// user | project | local
    pub scope: String,
    /// True when a subagent definition asks for this memory.
    pub declared: bool,
    pub file: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct MemoryStore {
    pub key: String,
    /// "project" | "user" | "agent" | "other"
    pub kind: String,
    pub label: String,
    pub dir: String,
    pub exists: bool,
    pub project_dir: Option<String>,
    pub enabled: bool,
    pub enabled_source: String,
    pub custom_dir: bool,
    pub agent: Option<AgentInfo>,
    pub index: MemoryIndex,
    pub entries: Vec<MemoryEntry>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct MemoryOverview {
    pub stores: Vec<MemoryStore>,
    pub types: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]*)\]\(([^)\s]+)\)").expect("valid regex"))
}

fn mtime_of(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `type`, or `metadata.type` as Claude Code has often written it.
fn entry_type(fm: &serde_yaml::Value) -> Option<String> {
    yaml_str(fm, "type").or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "type")))
}

fn read_index(dir: &Path) -> MemoryIndex {
    let path = dir.join("MEMORY.md");
    let content = read_content(&path);
    let text = content.text.clone().unwrap_or_default();
    let loaded = memory_loaded_bytes(&text);
    let links = link_re()
        .captures_iter(&text)
        .filter_map(|c| {
            let target = c[2].to_string();
            if target.contains("://") || target.starts_with('#') {
                return None;
            }
            let resolved = dir.join(target.replace('/', std::path::MAIN_SEPARATOR_STR));
            Some(IndexLink {
                title: c[1].trim().to_string(),
                exists: resolved.is_file(),
                path: s(&resolved),
                target,
            })
        })
        .collect();
    MemoryIndex {
        exists: path.is_file(),
        path: s(&path),
        bytes: content.bytes,
        lines: content.lines,
        loaded_bytes: loaded,
        truncated: loaded < content.bytes,
        links,
    }
}

fn read_entries(dir: &Path, index: &MemoryIndex) -> Vec<MemoryEntry> {
    let index_path = PathBuf::from(&index.path);
    let index_text = fs::read_to_string(&index_path).unwrap_or_default();
    let mut entries: Vec<MemoryEntry> = md_files(dir)
        .into_iter()
        .filter(|p| path_key(p) != path_key(&index_path))
        .map(|p| {
            let content = read_content(&p);
            let text = content.text.clone().unwrap_or_default();
            let fm = frontmatter(&text);
            let rel = rel_label(&p, dir);
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let name_in_index = index.links.iter().any(|l| path_key(Path::new(&l.path)) == path_key(&p));
            // Some indexes name the file without a markdown link.
            let mentioned = index_text.contains(&rel) || index_text.contains(&stem);
            MemoryEntry {
                name: fm.as_ref().and_then(|f| yaml_str(f, "name")).unwrap_or_else(|| stem.clone()),
                description: fm.as_ref().and_then(|f| yaml_str(f, "description")),
                kind: fm.as_ref().and_then(entry_type),
                modified: fm.as_ref().and_then(|f| yaml_str(f, "modified")),
                has_frontmatter: fm.is_some(),
                mtime: mtime_of(&p),
                bytes: content.bytes,
                lines: content.lines,
                in_index: name_in_index || mentioned,
                rel,
                path: s(&p),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn store_warnings(index: &MemoryIndex, entries: &[MemoryEntry]) -> Vec<String> {
    let mut out = Vec::new();
    if !index.exists && !entries.is_empty() {
        out.push(format!(
            "No MEMORY.md — Claude loads only the index at startup, so these {} notes may never be found.",
            entries.len()
        ));
    }
    if index.truncated {
        out.push(format!(
            "MEMORY.md is longer than the {MEMORY_MAX_LINES}-line / 25 KB limit — everything past that is dropped at startup."
        ));
    }
    let missing = entries.iter().filter(|e| !e.in_index).count();
    if index.exists && missing > 0 {
        out.push(format!("{missing} note(s) aren't mentioned in MEMORY.md."));
    }
    for l in index.links.iter().filter(|l| !l.exists) {
        out.push(format!("MEMORY.md links to {}, which doesn't exist.", l.target));
    }
    out
}

fn build_store(
    key: String,
    kind: &str,
    label: String,
    dir: PathBuf,
    project_dir: Option<&Path>,
    agent: Option<AgentInfo>,
    enabled: (bool, String),
    custom_dir: bool,
) -> MemoryStore {
    let index = read_index(&dir);
    let entries = if dir.is_dir() { read_entries(&dir, &index) } else { Vec::new() };
    MemoryStore {
        key,
        kind: kind.into(),
        label,
        exists: dir.is_dir(),
        project_dir: project_dir.map(|p| s(p)),
        enabled: enabled.0,
        enabled_source: enabled.1,
        custom_dir,
        agent,
        warnings: store_warnings(&index, &entries),
        index,
        entries,
        dir: s(&dir),
    }
}

/// Subagent definitions that ask for memory: name -> scope.
fn declared_agent_memory(roots: &[PathBuf]) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    for root in roots {
        for file in md_files(root) {
            let Ok(text) = fs::read_to_string(&file) else { continue };
            let Some(fm) = frontmatter(&text) else { continue };
            let Some(scope) = yaml_str(&fm, "memory") else { continue };
            if !AGENT_SCOPES.iter().any(|(v, _)| *v == scope) {
                continue;
            }
            let name = yaml_str(&fm, "name").unwrap_or_else(|| {
                file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            });
            out.push((name, scope, file));
        }
    }
    out
}

/// Subagent memory folders under `root`, plus folders declared but not
/// created yet.
fn agent_stores(
    root: &Path,
    scope: &str,
    rel: &str,
    project: Option<&Path>,
    agents_dirs: &[PathBuf],
    enabled: (bool, String),
) -> Vec<MemoryStore> {
    let base = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let declared = declared_agent_memory(agents_dirs);
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let existing: Vec<PathBuf> = fs::read_dir(&base)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    for dir in existing {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        seen.insert(name.to_lowercase());
        let decl = declared.iter().find(|(n, sc, _)| n.eq_ignore_ascii_case(&name) && sc == scope);
        out.push(build_store(
            format!("agent:{scope}:{}", path_key(&dir)),
            "agent",
            name.clone(),
            dir,
            project,
            Some(AgentInfo {
                name,
                scope: scope.into(),
                declared: decl.is_some(),
                file: decl.map(|(_, _, f)| s(f)),
            }),
            enabled.clone(),
            false,
        ));
    }
    // Declared but never written to yet.
    for (name, sc, file) in declared.iter().filter(|(_, sc, _)| sc == scope) {
        if seen.contains(&name.to_lowercase()) {
            continue;
        }
        let dir = base.join(name);
        out.push(build_store(
            format!("agent:{sc}:{}", path_key(&dir)),
            "agent",
            name.clone(),
            dir,
            project,
            Some(AgentInfo {
                name: name.clone(),
                scope: sc.clone(),
                declared: true,
                file: Some(s(file)),
            }),
            enabled.clone(),
            false,
        ));
    }
    out
}

pub fn overview() -> Result<MemoryOverview, String> {
    let home = home_dir()?;
    let claude = claude_dir()?;
    let user_scope = scope_files(&claude);
    let managed: Vec<(PathBuf, &str)> = crate::hooks::managed_settings_files()
        .into_iter()
        .map(|f| (f, "managed policy"))
        .collect();
    let mut user_layers = managed.clone();
    user_layers.push((user_scope[1].clone(), "user settings.local.json"));
    user_layers.push((user_scope[0].clone(), "user settings.json"));
    let user_state = auto_memory_state(&user_layers, claude.join("projects"), &home);
    let user_enabled = (user_state.enabled, user_state.source.clone());

    let mut stores = Vec::new();
    let mut claimed: HashSet<String> = HashSet::new();

    if user_state.custom_dir {
        stores.push(build_store(
            "user-custom".into(),
            "user",
            "Custom memory folder".into(),
            PathBuf::from(&user_state.dir),
            None,
            None,
            user_enabled.clone(),
            true,
        ));
        claimed.insert(path_key(Path::new(&user_state.dir)));
    }

    // Per-project auto memory, then that project's subagent memory.
    for project in projects() {
        let pclaude = project.join(".claude");
        let pscope = scope_files(&pclaude);
        let mut layers = managed.clone();
        layers.push((pscope[1].clone(), "project settings.local.json"));
        layers.push((pscope[0].clone(), "project settings.json"));
        layers.extend(user_layers.iter().skip(managed.len()).cloned());
        let state = auto_memory_state(
            &layers,
            claude.join("projects").join(encode_project(&git_root(&project))).join("memory"),
            &home,
        );
        let enabled = (state.enabled, state.source.clone());
        let dir = PathBuf::from(&state.dir);
        if dir.is_dir() && claimed.insert(path_key(&dir)) {
            stores.push(build_store(
                format!("project:{}", project.display()),
                "project",
                project_label(&project),
                dir,
                Some(&project),
                None,
                enabled.clone(),
                state.custom_dir,
            ));
        }
        let agents = [claude.join("agents"), pclaude.join("agents")];
        for (scope, rel) in AGENT_SCOPES.iter().filter(|(sc, _)| *sc != "user") {
            for store in agent_stores(&project, scope, rel, Some(&project), &agents, enabled.clone()) {
                if claimed.insert(path_key(Path::new(&store.dir))) {
                    stores.push(store);
                }
            }
        }
    }

    // User-scope subagent memory.
    for store in agent_stores(&claude, "user", "agent-memory", None, &[claude.join("agents")], user_enabled.clone())
    {
        if claimed.insert(path_key(Path::new(&store.dir))) {
            stores.push(store);
        }
    }

    // Memory folders that match no registered project (old worktrees…).
    if let Ok(rd) = fs::read_dir(claude.join("projects")) {
        let mut dirs: Vec<PathBuf> =
            rd.filter_map(|e| e.ok()).map(|e| e.path().join("memory")).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for dir in dirs {
            if !claimed.insert(path_key(&dir)) {
                continue;
            }
            let label = dir
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            stores.push(build_store(
                format!("other:{}", path_key(&dir)),
                "other",
                label,
                dir,
                None,
                None,
                user_enabled.clone(),
                false,
            ));
        }
    }

    Ok(MemoryOverview { stores, types: MEMORY_TYPES.to_vec() })
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// Memory folders the editor may read and write.
pub fn is_allowed(path: &Path) -> bool {
    let Ok(p) = path.canonicalize().map(|c| strip_verbatim(&c)) else { return false };
    let Ok(claude) = claude_dir().map(|c| canon(&c)) else { return false };
    // Folders are allowed (creating a note needs its directory); files must
    // be markdown.
    let is_markdown = p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false);
    if !p.is_dir() && !is_markdown {
        return false;
    }
    if under(&p, &claude.join("agent-memory")) {
        return true;
    }
    if under(&p, &claude.join("projects")) && p.components().any(|c| c.as_os_str() == "memory") {
        return true;
    }
    projects().iter().any(|project| {
        let project = canon(project);
        AGENT_SCOPES
            .iter()
            .any(|(_, rel)| under(&p, &project.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))))
    })
}

fn guard(path: &Path) -> Result<(), String> {
    if is_allowed(path) {
        Ok(())
    } else {
        Err(format!("{} is not inside a memory folder this app manages", path.display()))
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn index_line(name: &str, file: &str, description: Option<&str>) -> String {
    match description.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("- [{name}]({file}) — {d}\n"),
        None => format!("- [{name}]({file})\n"),
    }
}

/// Append a line for `file` to the store's MEMORY.md, creating it if needed.
pub fn add_to_index(dir: &Path, file: &str, name: &str, description: Option<&str>) -> Result<(), String> {
    let index = dir.join("MEMORY.md");
    guard(dir)?;
    let mut text = fs::read_to_string(&index).unwrap_or_else(|_| "# Memory Index\n\n".to_string());
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&index_line(name, file, description));
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    fs::write(&index, text).map_err(|e| format!("cannot write {}: {e}", index.display()))
}

/// Drop index lines that link to (or name) `file`.
fn remove_from_index(dir: &Path, file: &str) -> Result<bool, String> {
    let index = dir.join("MEMORY.md");
    let Ok(text) = fs::read_to_string(&index) else { return Ok(false) };
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| {
            let links_here = link_re().captures_iter(line).any(|c| c[2].trim() == file);
            !(links_here || (line.trim_start().starts_with('-') && line.contains(file)))
        })
        .collect();
    if kept.len() == text.lines().count() {
        return Ok(false);
    }
    let mut out = kept.join("\n");
    out.push('\n');
    fs::write(&index, out).map_err(|e| format!("cannot write {}: {e}", index.display()))?;
    Ok(true)
}

/// Write a new memory note (and optionally index it). Returns its path.
pub fn create_entry(
    dir: &str,
    name: &str,
    description: &str,
    kind: &str,
    body: &str,
    index: bool,
) -> Result<String, String> {
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        // New stores (a declared subagent that never ran) start empty.
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    guard(&dir)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("give the note a name".into());
    }
    let slug = slugify(name);
    if slug.is_empty() {
        return Err("the name needs at least one letter or digit".into());
    }
    if !MEMORY_TYPES.contains(&kind) {
        return Err(format!("type must be one of {}", MEMORY_TYPES.join(", ")));
    }
    let file = dir.join(format!("{slug}.md"));
    if file.exists() {
        return Err(format!("{} already exists", file.display()));
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let text = format!(
        "---\nname: {slug}\ndescription: {}\ntype: {kind}\nmodified: {}\n---\n\n{}\n",
        description.trim().replace('\n', " "),
        iso_date(now),
        body.trim_end()
    );
    fs::write(&file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    if index {
        add_to_index(&dir, &format!("{slug}.md"), name, Some(description))?;
    }
    Ok(s(&file))
}

/// Epoch seconds as an ISO 8601 date-time (UTC), matching Claude Code's
/// `modified` field closely enough to be readable.
fn iso_date(epoch: u64) -> String {
    let days = epoch / 86_400;
    let secs = epoch % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

pub fn delete_entry(path: &str, from_index: bool) -> Result<String, String> {
    let file = PathBuf::from(path);
    guard(&file)?;
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    let dir = file.parent().ok_or("no parent folder")?.to_path_buf();
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    fs::remove_file(&file).map_err(|e| format!("cannot delete {path}: {e}"))?;
    let unindexed = from_index && remove_from_index(&dir, &name)?;
    Ok(if unindexed {
        format!("Deleted {name} and its index line")
    } else {
        format!("Deleted {name}")
    })
}

/// Add an existing note to its store's index.
pub fn index_entry(path: &str) -> Result<String, String> {
    let file = PathBuf::from(path);
    guard(&file)?;
    let dir = file.parent().ok_or("no parent folder")?.to_path_buf();
    let rel = rel_label(&file, &dir);
    let content = read_content(&file);
    let text = content.text.unwrap_or_default();
    let fm = frontmatter(&text);
    let name = fm
        .as_ref()
        .and_then(|f| yaml_str(f, "name"))
        .unwrap_or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    let description = fm.as_ref().and_then(|f| yaml_str(f, "description"));
    add_to_index(&dir, &rel, &name, description.as_deref())?;
    Ok(format!("Added {rel} to MEMORY.md"))
}

/// Stores to copy into the tracking repo, as (repo-relative dir, files).
pub fn snapshot_dirs(stores: &[MemoryStore]) -> Vec<(String, PathBuf)> {
    stores
        .iter()
        .filter(|s| s.exists)
        .map(|store| {
            let dir = PathBuf::from(&store.dir);
            let rel = match store.kind.as_str() {
                "project" => format!("projects/{}", store.label),
                "agent" => {
                    let scope = store.agent.as_ref().map(|a| a.scope.clone()).unwrap_or_default();
                    format!("agents/{scope}/{}", store.label)
                }
                "user" => "user".to_string(),
                _ => format!("other/{}", store.label),
            };
            (rel, dir)
        })
        .collect()
}

/// Files of a memory dir worth snapshotting.
pub fn store_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "skills-editor-mem-{tag}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn slugs_and_index_lines() {
        assert_eq!(slugify("Set-Content positional binding!"), "set-content-positional-binding");
        assert_eq!(slugify("  Hello   World  "), "hello-world");
        assert_eq!(slugify("***"), "");
        assert_eq!(index_line("T", "t.md", Some("why")), "- [T](t.md) — why\n");
        assert_eq!(index_line("T", "t.md", Some("  ")), "- [T](t.md)\n");
    }

    #[test]
    fn iso_dates_match_known_instants() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_date(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_date(1_767_225_600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn reads_both_frontmatter_shapes_and_index_state() {
        let dir = scratch("read").join("memory");
        write(
            &dir.join("MEMORY.md"),
            "# Memory Index\n\n- [Docs style](docs-style.md) — how to write\n- [Gone](gone.md) — missing\n",
        );
        write(&dir.join("docs-style.md"), "---\nname: docs-style\ndescription: how to write\ntype: feedback\n---\nbody\n");
        write(
            &dir.join("legacy.md"),
            "---\nname: legacy\ndescription: old shape\nmetadata:\n  type: reference\n  node_type: memory\n---\nbody\n",
        );
        write(&dir.join("bare.md"), "just text\n");

        let index = read_index(&dir);
        assert!(index.exists && !index.truncated);
        assert_eq!(index.links.len(), 2);
        assert!(index.links[0].exists);
        assert!(!index.links[1].exists);

        let entries = read_entries(&dir, &index);
        let by = |n: &str| entries.iter().find(|e| e.rel == n).unwrap().clone();
        assert_eq!(by("docs-style.md").kind.as_deref(), Some("feedback"));
        assert!(by("docs-style.md").in_index);
        assert_eq!(by("legacy.md").kind.as_deref(), Some("reference"), "metadata.type is read");
        assert!(!by("legacy.md").in_index);
        assert_eq!(by("bare.md").name, "bare");
        assert!(!by("bare.md").has_frontmatter);
        assert!(by("bare.md").mtime > 0);

        let warnings = store_warnings(&index, &entries);
        assert!(warnings.iter().any(|w| w.contains("2 note(s) aren't mentioned")));
        assert!(warnings.iter().any(|w| w.contains("links to gone.md")));

        // A long index is reported as truncated.
        write(&dir.join("MEMORY.md"), &"- x\n".repeat(300));
        assert!(read_index(&dir).truncated);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn create_index_and_delete_round_trip() {
        let home = scratch("ops");
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let dir = home.join(".claude").join("projects").join("P").join("memory");
        fs::create_dir_all(&dir).unwrap();
        write(&home.join(".claude.json"), "{\"projects\":{}}");

        let path = create_entry(&s(&dir), "Deploy needs VPN", "vpn first", "project", "Body here", true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("---\nname: deploy-needs-vpn\ndescription: vpn first\ntype: project\nmodified: 20"));
        assert!(text.trim_end().ends_with("Body here"));
        let index = fs::read_to_string(dir.join("MEMORY.md")).unwrap();
        assert!(index.starts_with("# Memory Index"));
        assert!(index.contains("- [Deploy needs VPN](deploy-needs-vpn.md) — vpn first"));

        assert!(create_entry(&s(&dir), "Deploy needs VPN", "", "project", "", false).is_err());
        assert!(create_entry(&s(&dir), "!!!", "", "project", "", false).is_err());
        assert!(create_entry(&s(&dir), "ok", "", "bogus", "", false).is_err());

        // An unindexed note can be added later.
        write(&dir.join("manual.md"), "---\nname: manual\ndescription: added by hand\n---\nx\n");
        index_entry(&s(&dir.join("manual.md"))).unwrap();
        assert!(fs::read_to_string(dir.join("MEMORY.md")).unwrap().contains("- [manual](manual.md) — added by hand"));

        // Deleting can drop the index line too.
        let msg = delete_entry(&path, true).unwrap();
        assert!(msg.contains("index line"));
        let index = fs::read_to_string(dir.join("MEMORY.md")).unwrap();
        assert!(!index.contains("deploy-needs-vpn"));
        assert!(index.contains("manual.md"), "other lines survive");
        assert!(!Path::new(&path).exists());

        // Outside a memory folder: refused.
        write(&home.join("loose.md"), "x");
        assert!(delete_entry(&s(&home.join("loose.md")), false).is_err());
        assert!(home.join("loose.md").is_file());

        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn overview_finds_project_agent_and_stray_stores() {
        let home = scratch("ov");
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let claude = home.join(".claude");
        let proj = home.join("app");
        fs::create_dir_all(proj.join(".git")).unwrap();
        write(
            &home.join(".claude.json"),
            &serde_json::json!({ "projects": { (s(&proj).replace('\\', "/")): {} } }).to_string(),
        );
        let auto = claude.join("projects").join(encode_project(&proj)).join("memory");
        write(&auto.join("MEMORY.md"), "- [a](a.md)\n");
        write(&auto.join("a.md"), "---\nname: a\ntype: user\n---\nx\n");
        write(&claude.join("projects").join("C--stray").join("memory").join("MEMORY.md"), "old\n");
        // A subagent that declares memory but has never written any.
        write(&claude.join("agents").join("explorer.md"), "---\nname: explorer\ndescription: d\nmemory: user\n---\nx\n");
        // A project-scoped agent memory folder that exists.
        write(&proj.join(".claude/agent-memory/reviewer/MEMORY.md"), "- [n](n.md)\n");

        let ov = overview().unwrap();
        let find = |kind: &str, label: &str| {
            ov.stores.iter().find(|s| s.kind == kind && s.label == label).unwrap_or_else(|| panic!("no {kind} {label}")).clone()
        };
        let app = find("project", "app");
        assert!(app.exists && app.enabled && app.entries.len() == 1);
        assert_eq!(app.entries[0].kind.as_deref(), Some("user"));
        let reviewer = find("agent", "reviewer");
        assert_eq!(reviewer.agent.as_ref().unwrap().scope, "project");
        assert!(!reviewer.agent.as_ref().unwrap().declared, "no definition asks for it");
        let explorer = find("agent", "explorer");
        assert!(explorer.agent.as_ref().unwrap().declared);
        assert!(!explorer.exists, "declared but never written");
        assert!(find("other", "C--stray").exists);

        // Turning auto memory off is reflected on every store.
        write(&claude.join("settings.json"), "{\"autoMemoryEnabled\":false}");
        let ov = overview().unwrap();
        assert!(ov.stores.iter().all(|s| !s.enabled));
        assert!(ov.stores.iter().all(|s| s.enabled_source.contains("user settings.json")));

        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = fs::remove_dir_all(home);
    }
}
