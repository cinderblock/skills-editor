use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::discovery::{discover, SkillGroup};
use crate::files::remove_dir_all_robust;
use crate::git;
use crate::hooks::{self, HookGroup};
use crate::instructions::{self, InstrFile, InstrGroup};
use crate::memory;
use crate::settings::Settings;

#[derive(Serialize)]
pub struct SyncStatus {
    pub repo_path: String,
    pub initialized: bool,
    pub hostname: String,
    pub branch: Option<String>,
    pub on_host_branch: bool,
    pub dirty: bool,
    pub last_commit: Option<String>,
    pub branches: Vec<String>,
    pub remote: Option<String>,
}

/// Hostname sanitized into a valid git branch name.
pub fn host_branch() -> String {
    let raw = gethostname::gethostname().to_string_lossy().to_lowercase();
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim_matches(['-', '.']).to_string();
    if cleaned.is_empty() { "unknown-host".into() } else { cleaned }
}

pub fn status(settings: &Settings) -> Result<SyncStatus, String> {
    let repo = settings.effective_repo_path()?;
    let hostname = host_branch();
    if !git::is_repo(&repo) {
        return Ok(SyncStatus {
            repo_path: repo.to_string_lossy().to_string(),
            initialized: false,
            hostname,
            branch: None,
            on_host_branch: false,
            dirty: false,
            last_commit: None,
            branches: Vec::new(),
            remote: None,
        });
    }
    let branch = git::run(&repo, &["branch", "--show-current"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let dirty = git::run(&repo, &["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let last_commit = git::run(&repo, &["log", "-1", "--format=%h %s (%cr)"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let branches = git::run(&repo, &["branch", "-a", "--format=%(refname:short)"])
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.ends_with("/HEAD"))
                .collect()
        })
        .unwrap_or_default();
    let remote = git::run(&repo, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(SyncStatus {
        repo_path: repo.to_string_lossy().to_string(),
        initialized: true,
        on_host_branch: branch.as_deref() == Some(hostname.as_str()),
        hostname,
        branch,
        dirty,
        last_commit,
        branches,
        remote,
    })
}

pub fn init(settings: &Settings) -> Result<String, String> {
    let repo = settings.effective_repo_path()?;
    if git::is_repo(&repo) {
        return Err(format!("{} is already a git repo", repo.display()));
    }
    fs::create_dir_all(&repo).map_err(|e| format!("cannot create repo dir: {e}"))?;
    let branch = host_branch();
    git::run_in(&repo, &["init", "-b", &branch, "."])?;
    let readme = repo.join("README.md");
    if !readme.exists() {
        let text = "# Skills tracking repo\n\n\
            Managed by Skills Editor. Each host commits snapshots of its skills to a\n\
            branch named after the host. Layout:\n\n\
            - `user/<skill>/` — user-level skills (`~/.claude/skills`)\n\
            - `projects/<project>/<skill>/` — per-project skills\n\
            - `extra/<root>/<skill>/` — skills from extra configured roots\n\
            - `hooks/user/`, `hooks/projects/<project>/` — hook config from each\n  \
            settings file, plus the scripts those hooks run (`scripts/`)\n\
            - `instructions/` — CLAUDE.md files, rules, and other agents' instruction\n  \
            files (user, per project, and parent folders)\n\
            - `memory/` — the notes Claude writes itself: `projects/<project>/`,\n  \
            `agents/<scope>/<agent>/`, and folders matching no project\n\
            - `manifest.json` — maps repo paths back to their source locations\n\n\
            Share skills between hosts by pushing to a common remote and\n\
            cherry-picking between host branches.\n";
        fs::write(&readme, text).map_err(|e| format!("cannot write README: {e}"))?;
    }
    if let Some(url) = settings.remote_url.as_deref().filter(|u| !u.trim().is_empty()) {
        git::run(&repo, &["remote", "add", "origin", url.trim()])?;
    }
    git::run(&repo, &["add", "-A"])?;
    git::run(&repo, &["commit", "-m", "Initialize skills tracking repo"])?;
    Ok(format!("Initialized {} on branch {branch}", repo.display()))
}

#[derive(Serialize)]
struct ManifestEntry {
    repo_dir: String,
    source: String,
    /// "skill" | "hooks" | "hook-script" | "disabled-hooks" | "instructions" | "memory"
    kind: &'static str,
    /// Display name (the skill name for skills; kept for older readers).
    skill: String,
}

#[derive(Serialize)]
struct Manifest {
    hostname: String,
    generated_at_epoch_secs: u64,
    entries: Vec<ManifestEntry>,
}

const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "__pycache__", ".venv"];

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            if name.to_str().map(|n| SKIP_DIRS.contains(&n)).unwrap_or(false) {
                continue;
            }
            copy_dir(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("cannot copy {}: {e}", src_path.display()))?;
        }
    }
    Ok(())
}

/// A group label that is safe and unique as a directory name.
fn unique_label(label: &str, taken: &mut Vec<String>) -> String {
    let base: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_. ".contains(c) { c } else { '-' })
        .collect::<String>()
        .trim()
        .to_string();
    let base = if base.is_empty() { "group".to_string() } else { base };
    let mut candidate = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    taken.push(candidate.clone());
    candidate
}

/// Write each user/project settings file's hook config and its scripts:
///
/// - `hooks/<scope>/<settings file>` — `{"hooks": …, "disableAllHooks": …}`
/// - `hooks/<scope>/scripts/…` — scripts from the scope's `.claude/hooks`
///   dir; referenced scripts elsewhere go under `scripts/external/`
/// - `hooks/disabled-hooks.json` — hooks disabled from the app
///
/// Managed, plugin and frontmatter hooks aren't this machine's own config
/// (frontmatter hooks travel with their skill). Returns files written.
fn snapshot_hooks(
    repo: &Path,
    hook_groups: &[HookGroup],
    project_dirs: &HashMap<String, String>,
    sidecar: &Path,
    manifest: &mut Manifest,
) -> Result<usize, String> {
    let mut written = 0usize;
    let rel = |p: &Path| p.strip_prefix(repo).unwrap_or(p).to_string_lossy().replace('\\', "/");
    for group in hook_groups {
        let scope = match group.kind.as_str() {
            "user" => repo.join("hooks").join("user"),
            "project" => repo.join("hooks").join("projects").join(&project_dirs[&group.key]),
            _ => continue,
        };
        for src in &group.sources {
            let has_hooks = src.hooks.as_object().map(|o| !o.is_empty()).unwrap_or(false);
            if !src.exists || !(has_hooks || src.disable_all_hooks) {
                continue;
            }
            let mut doc = serde_json::Map::new();
            if has_hooks {
                doc.insert("hooks".into(), src.hooks.clone());
            }
            if src.disable_all_hooks {
                doc.insert("disableAllHooks".into(), true.into());
            }
            let dst = scope.join(&src.file_label);
            fs::create_dir_all(&scope).map_err(|e| format!("cannot create {}: {e}", scope.display()))?;
            let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
            fs::write(&dst, text).map_err(|e| format!("cannot write {}: {e}", dst.display()))?;
            written += 1;
            manifest.entries.push(ManifestEntry {
                repo_dir: rel(&dst),
                source: src.file.clone(),
                kind: "hooks",
                skill: format!("{} hooks ({})", group.label, src.file_label),
            });
        }
        let hooks_dir = PathBuf::from(&group.detail).join("hooks");
        for script in &group.scripts {
            let path = PathBuf::from(&script.path);
            if !path.is_file() {
                continue;
            }
            let inside = path.strip_prefix(&hooks_dir).ok().map(Path::to_path_buf);
            let dst = match inside {
                Some(r) => scope.join("scripts").join(r),
                None => scope
                    .join("scripts")
                    .join("external")
                    .join(path.file_name().unwrap_or_default()),
            };
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            fs::copy(&path, &dst).map_err(|e| format!("cannot copy {}: {e}", path.display()))?;
            written += 1;
            manifest.entries.push(ManifestEntry {
                repo_dir: rel(&dst),
                source: script.path.clone(),
                kind: "hook-script",
                skill: script.name.clone(),
            });
        }
    }
    let parked = hooks::load_sidecar(sidecar)?;
    if !parked.hooks.is_empty() {
        let dst = repo.join("hooks").join("disabled-hooks.json");
        fs::create_dir_all(repo.join("hooks")).map_err(|e| e.to_string())?;
        fs::copy(sidecar, &dst).map_err(|e| format!("cannot copy disabled hooks: {e}"))?;
        written += 1;
        manifest.entries.push(ManifestEntry {
            repo_dir: rel(&dst),
            source: sidecar.to_string_lossy().to_string(),
            kind: "disabled-hooks",
            skill: format!("{} disabled hooks", parked.hooks.len()),
        });
    }
    Ok(written)
}

/// Where an instruction/memory file goes in the repo, if it's snapshotted.
///
/// - `instructions/user/…`, `instructions/projects/<project>/…`
/// - `instructions/parents/<encoded folder>/CLAUDE.md`
/// (Memory folders are copied separately by `snapshot_memory`.)
///
/// Managed policy is the administrator's, not this machine's own config.
fn instruction_dest(
    repo: &Path,
    group: &InstrGroup,
    file: &InstrFile,
    project_dirs: &HashMap<String, String>,
) -> Option<PathBuf> {
    let rel = |label: &str| -> Option<PathBuf> {
        let p = PathBuf::from(label.replace('/', std::path::MAIN_SEPARATOR_STR));
        // Labels are relative; never let one escape its folder.
        p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            .then_some(p)
    };
    match group.kind.as_str() {
        "user" => Some(repo.join("instructions").join("user").join(rel(&file.label)?)),
        "project" => {
            let name = project_dirs.get(&group.key)?;
            Some(repo.join("instructions").join("projects").join(name).join(rel(&file.label)?))
        }
        "parents" => {
            let path = Path::new(&file.path);
            Some(
                repo.join("instructions")
                    .join("parents")
                    .join(instructions::encode_project(path.parent()?))
                    .join(path.file_name()?),
            )
        }
        _ => None,
    }
}

/// Copy every memory store (auto memory and subagent memory) into `memory/`.
fn snapshot_memory(
    repo: &Path,
    stores: &[memory::MemoryStore],
    manifest: &mut Manifest,
) -> Result<usize, String> {
    let mut written = 0usize;
    let mut taken: Vec<String> = Vec::new();
    for (rel, dir) in memory::snapshot_dirs(stores) {
        // Store labels are project/agent names; keep them filesystem-safe.
        let safe: PathBuf = rel.split('/').map(|part| unique_part(part)).collect();
        let base = repo.join("memory").join(&safe);
        let key = safe.to_string_lossy().to_lowercase();
        if taken.contains(&key) {
            continue;
        }
        taken.push(key);
        for file in memory::store_files(&dir) {
            let Ok(rest) = file.strip_prefix(&dir) else { continue };
            let dst = base.join(rest);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            fs::copy(&file, &dst).map_err(|e| format!("cannot copy {}: {e}", file.display()))?;
            written += 1;
        }
        manifest.entries.push(ManifestEntry {
            repo_dir: base.strip_prefix(repo).unwrap_or(&base).to_string_lossy().replace('\\', "/"),
            source: dir.to_string_lossy().to_string(),
            kind: "memory",
            skill: rel,
        });
    }
    Ok(written)
}

/// One path component, safe on every filesystem.
fn unique_part(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_. ".contains(c) { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { "unnamed".into() } else { cleaned }
}

fn snapshot_instructions(
    repo: &Path,
    groups: &[InstrGroup],
    project_dirs: &HashMap<String, String>,
    manifest: &mut Manifest,
) -> Result<usize, String> {
    let mut written = 0usize;
    for group in groups {
        for file in &group.files {
            let Some(dst) = instruction_dest(repo, group, file, project_dirs) else { continue };
            let src = Path::new(&file.path);
            if !src.is_file() {
                continue;
            }
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            fs::copy(src, &dst).map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
            written += 1;
            manifest.entries.push(ManifestEntry {
                repo_dir: dst.strip_prefix(repo).unwrap_or(&dst).to_string_lossy().replace('\\', "/"),
                source: file.path.clone(),
                kind: if file.kind.starts_with("memory") { "memory" } else { "instructions" },
                skill: format!("{} — {}", group.label, file.label),
            });
        }
    }
    Ok(written)
}

pub fn snapshot(settings: &Settings, sidecar: &Path) -> Result<String, String> {
    let repo = settings.effective_repo_path()?;
    if !git::is_repo(&repo) {
        return Err("tracking repo is not initialized yet".into());
    }
    let branch = git::run(&repo, &["branch", "--show-current"])?.trim().to_string();
    let host = host_branch();
    if branch != host {
        return Err(format!(
            "repo is on branch '{branch}' but this host's branch is '{host}' — switch back before snapshotting"
        ));
    }

    let groups: Vec<SkillGroup> = discover(settings)?;
    let hook_groups = hooks::overview(settings, None)?.groups;
    let instr_groups = instructions::overview(true)?.groups;
    let mut manifest = Manifest {
        hostname: host.clone(),
        generated_at_epoch_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        entries: Vec::new(),
    };

    // Rebuild the snapshot dirs from scratch so deletions propagate.
    for sub in ["user", "projects", "extra", "hooks", "instructions", "memory"] {
        remove_dir_all_robust(&repo.join(sub))?;
    }

    // One directory name per project, shared by its skills and its hooks.
    // Skill projects are named first so their existing paths stay stable.
    let mut project_labels: Vec<String> = Vec::new();
    let mut project_dirs: HashMap<String, String> = HashMap::new();
    let project_groups = groups
        .iter()
        .filter(|g| g.kind == "project")
        .map(|g| (&g.key, &g.label))
        .chain(hook_groups.iter().filter(|g| g.kind == "project").map(|g| (&g.key, &g.label)))
        .chain(instr_groups.iter().filter(|g| g.kind == "project").map(|g| (&g.key, &g.label)));
    for (key, label) in project_groups {
        if !project_dirs.contains_key(key) {
            project_dirs.insert(key.clone(), unique_label(label, &mut project_labels));
        }
    }

    let mut skill_count = 0usize;
    let mut extra_labels: Vec<String> = Vec::new();
    for group in &groups {
        let base: Option<PathBuf> = match group.kind.as_str() {
            "user" => Some(repo.join("user")),
            "project" => Some(repo.join("projects").join(&project_dirs[&group.key])),
            "extra" => Some(
                repo.join("extra")
                    .join(unique_label(&group.label, &mut extra_labels)),
            ),
            // Plugin skills are marketplace-managed artifacts, not the
            // machine's own evolution — excluded from snapshots.
            _ => None,
        };
        let Some(base) = base else { continue };
        for skill in &group.skills {
            let src = PathBuf::from(&skill.dir);
            let skill_dir_name = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| skill.name.clone());
            let dst = base.join(&skill_dir_name);
            copy_dir(&src, &dst)?;
            skill_count += 1;
            manifest.entries.push(ManifestEntry {
                repo_dir: dst
                    .strip_prefix(&repo)
                    .unwrap_or(&dst)
                    .to_string_lossy()
                    .replace('\\', "/"),
                source: skill.dir.clone(),
                kind: "skill",
                skill: skill.name.clone(),
            });
        }
    }

    let hook_count = snapshot_hooks(&repo, &hook_groups, &project_dirs, sidecar, &mut manifest)?;
    let instr_count = snapshot_instructions(&repo, &instr_groups, &project_dirs, &mut manifest)?
        + snapshot_memory(&repo, &memory::overview()?.stores, &mut manifest)?;

    let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(repo.join("manifest.json"), manifest_text)
        .map_err(|e| format!("cannot write manifest: {e}"))?;

    git::run(&repo, &["add", "-A"])?;
    let porcelain = git::run(&repo, &["status", "--porcelain"])?;
    if porcelain.trim().is_empty() {
        return Ok("No changes since last snapshot".into());
    }
    let changed = porcelain.lines().count();
    let msg = format!(
        "Snapshot from {host}: {skill_count} skills, {hook_count} hook files, {instr_count} instruction/memory files, {changed} files changed"
    );
    git::run(&repo, &["commit", "-m", &msg])?;
    let summary = git::run(&repo, &["log", "-1", "--format=%h %s"])?;
    Ok(summary.trim().to_string())
}

pub fn push(settings: &Settings) -> Result<String, String> {
    let repo = settings.effective_repo_path()?;
    if !git::is_repo(&repo) {
        return Err("tracking repo is not initialized yet".into());
    }
    // Configure origin from settings if it isn't set yet.
    let has_remote = git::run(&repo, &["remote", "get-url", "origin"]).is_ok();
    if !has_remote {
        match settings.remote_url.as_deref().filter(|u| !u.trim().is_empty()) {
            Some(url) => {
                git::run(&repo, &["remote", "add", "origin", url.trim()])?;
            }
            None => return Err("no remote configured — set one in Settings first".into()),
        }
    }
    let branch = git::run(&repo, &["branch", "--show-current"])?.trim().to_string();
    if branch.is_empty() {
        return Err("repo is in detached HEAD state; check out a branch first".into());
    }
    // Plain push only — never any kind of force.
    git::run(&repo, &["push", "-u", "origin", &branch])?;
    Ok(format!("Pushed {branch} to origin"))
}

pub fn fetch(settings: &Settings) -> Result<String, String> {
    let repo = settings.effective_repo_path()?;
    if !git::is_repo(&repo) {
        return Err("tracking repo is not initialized yet".into());
    }
    git::run(&repo, &["fetch", "--all", "--prune"])?;
    Ok("Fetched all remotes".into())
}

pub fn pull(settings: &Settings) -> Result<String, String> {
    let repo = settings.effective_repo_path()?;
    if !git::is_repo(&repo) {
        return Err("tracking repo is not initialized yet".into());
    }
    let out = git::run(&repo, &["pull", "--ff-only"])?;
    Ok(if out.trim().is_empty() { "Pulled".into() } else { out.trim().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instr_file(label: &str, path: &str, kind: &str) -> InstrFile {
        InstrFile {
            path: path.into(),
            label: label.into(),
            kind: kind.into(),
            loads: "startup".into(),
            editable: true,
            bytes: 0,
            lines: 0,
            excluded: false,
            paths: vec![],
            agent: None,
            imported_by: vec![],
            applies_to: vec![],
            imports: vec![],
            warnings: vec![],
        }
    }

    fn instr_group(kind: &str, key: &str) -> InstrGroup {
        InstrGroup {
            key: key.into(),
            kind: kind.into(),
            label: "g".into(),
            detail: String::new(),
            project_dir: None,
            files: vec![],
            startup: vec![],
            auto_memory: None,
            notes: vec![],
        }
    }

    #[test]
    fn instruction_files_map_into_the_repo_and_cannot_escape() {
        let repo = Path::new("/repo");
        let dirs: HashMap<String, String> = [("project:/w/app".to_string(), "app".to_string())].into();
        let project = instr_group("project", "project:/w/app");
        let dest = |g: &InstrGroup, label: &str, path: &str| {
            instruction_dest(repo, g, &instr_file(label, path, "claude"), &dirs)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        };
        assert_eq!(dest(&project, ".claude/rules/a.md", "x").as_deref(), Some("/repo/instructions/projects/app/.claude/rules/a.md"));
        assert_eq!(dest(&instr_group("user", "user"), "rules/x.md", "x").as_deref(), Some("/repo/instructions/user/rules/x.md"));
        assert_eq!(
            dest(&instr_group("parents", "parents"), "/w/CLAUDE.md", "/w/CLAUDE.md").as_deref(),
            Some("/repo/instructions/parents/-w/CLAUDE.md")
        );
        assert_eq!(dest(&project, "../escape.md", "x"), None);
        assert_eq!(dest(&instr_group("project", "project:/unknown"), "CLAUDE.md", "x"), None);
        assert_eq!(dest(&instr_group("managed", "managed"), "CLAUDE.md", "x"), None);
    }

    #[test]
    fn hook_snapshot_writes_config_scripts_and_parked_hooks() {
        let root = std::env::temp_dir().join(format!(
            "skills-editor-sync-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let home = root.join("home");
        let repo = root.join("repo");
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let claude = home.join(".claude");
        fs::create_dir_all(claude.join("hooks").join("lib")).unwrap();
        fs::write(claude.join("hooks/guard.sh"), "echo guard").unwrap();
        fs::write(claude.join("hooks/lib/util.sh"), "echo util").unwrap();
        fs::create_dir_all(home.join("tools")).unwrap();
        fs::write(home.join("tools/ext.py"), "print()").unwrap();
        fs::write(
            claude.join("settings.json"),
            r#"{"model":"x","disableAllHooks":true,"hooks":{"Stop":[{"hooks":[
                {"type":"command","command":"bash ~/.claude/hooks/guard.sh"},
                {"type":"command","command":"python ~/tools/ext.py"}]}]}}"#,
        )
        .unwrap();
        let sidecar = root.join("disabled-hooks.json");
        fs::write(&sidecar, r#"{"version":1,"hooks":[{"id":"a","file":"f","event":"Stop","handler":{"type":"prompt","prompt":"p"},"disabled_at":1}]}"#).unwrap();

        let groups = hooks::overview(&Settings::default(), None).unwrap().groups;
        let mut manifest = Manifest { hostname: "h".into(), generated_at_epoch_secs: 0, entries: Vec::new() };
        let written = snapshot_hooks(&repo, &groups, &HashMap::new(), &sidecar, &mut manifest).unwrap();
        crate::paths::TEST_HOME.with(|h| *h.borrow_mut() = None);

        let user = repo.join("hooks").join("user");
        let cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(user.join("settings.json")).unwrap()).unwrap();
        assert_eq!(cfg["disableAllHooks"], true);
        assert_eq!(cfg["hooks"]["Stop"][0]["hooks"].as_array().unwrap().len(), 2);
        assert!(cfg.get("model").is_none(), "only hook keys are exported");
        assert_eq!(fs::read_to_string(user.join("scripts/guard.sh")).unwrap(), "echo guard");
        assert_eq!(fs::read_to_string(user.join("scripts/lib/util.sh")).unwrap(), "echo util");
        assert_eq!(fs::read_to_string(user.join("scripts/external/ext.py")).unwrap(), "print()");
        assert!(repo.join("hooks/disabled-hooks.json").is_file());
        assert_eq!(written, 5);
        let kinds: Vec<&str> = manifest.entries.iter().map(|e| e.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "hook-script").count(), 3);
        assert!(kinds.contains(&"hooks") && kinds.contains(&"disabled-hooks"));
        let _ = fs::remove_dir_all(root);
    }
}
