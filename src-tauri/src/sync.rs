use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::discovery::{discover, SkillGroup};
use crate::files::remove_dir_all_robust;
use crate::git;
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

pub fn snapshot(settings: &Settings) -> Result<String, String> {
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
    let mut manifest = Manifest {
        hostname: host.clone(),
        generated_at_epoch_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        entries: Vec::new(),
    };

    // Rebuild the snapshot dirs from scratch so deletions propagate.
    for sub in ["user", "projects", "extra"] {
        remove_dir_all_robust(&repo.join(sub))?;
    }

    let mut skill_count = 0usize;
    let mut project_labels: Vec<String> = Vec::new();
    let mut extra_labels: Vec<String> = Vec::new();
    for group in &groups {
        let base: Option<PathBuf> = match group.kind.as_str() {
            "user" => Some(repo.join("user")),
            "project" => Some(
                repo.join("projects")
                    .join(unique_label(&group.label, &mut project_labels)),
            ),
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
                skill: skill.name.clone(),
            });
        }
    }

    let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(repo.join("manifest.json"), manifest_text)
        .map_err(|e| format!("cannot write manifest: {e}"))?;

    git::run(&repo, &["add", "-A"])?;
    let porcelain = git::run(&repo, &["status", "--porcelain"])?;
    if porcelain.trim().is_empty() {
        return Ok("No changes since last snapshot".into());
    }
    let changed = porcelain.lines().count();
    let msg = format!("Snapshot from {host}: {skill_count} skills, {changed} files changed");
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
