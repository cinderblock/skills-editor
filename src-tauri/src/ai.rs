use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Serialize, Clone)]
pub struct JobInfo {
    pub id: u64,
    pub label: String,
    /// Directory the job runs in (and may edit files under).
    pub cwd: String,
    pub prompt: String,
    /// Model passed to the CLI, or None for the user's default.
    pub model: Option<String>,
    /// Files (relative to cwd) written after parsing the structured response.
    pub applied_files: Vec<String>,
    /// Summary the model returned alongside its changes.
    pub notes: Option<String>,
    pub status: JobStatus,
    /// Last chunk of combined output, for list views.
    pub output_tail: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

struct Job {
    info: JobInfo,
    output: Arc<Mutex<String>>,
    child: Option<Arc<Mutex<Child>>>,
}

#[derive(Default)]
pub struct JobManager {
    next_id: AtomicU64,
    jobs: Mutex<HashMap<u64, Job>>,
}

pub type JobState = Arc<JobManager>;

pub fn new_state() -> JobState {
    Arc::new(JobManager::default())
}

/// Jobs run read-only and answer with this JSON shape; the app applies it.
/// (`~/.claude` is a protected dir for the claude CLI, so letting the agent
/// edit files directly fails — see AiTaskDialog prompt templates.)
#[derive(serde::Deserialize)]
struct StructuredResponse {
    #[serde(default)]
    files: Vec<FileChange>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(serde::Deserialize)]
struct FileChange {
    path: String,
    content: String,
}

/// Lenient parse: raw JSON, a ```json fence, or the outermost {...} span.
fn parse_response(output: &str) -> Result<StructuredResponse, String> {
    let t = output.trim();
    if let Ok(r) = serde_json::from_str::<StructuredResponse>(t) {
        return Ok(r);
    }
    if let Some(start) = t.find("```") {
        let after = &t[start..];
        let body_start = after.find('\n').map(|i| start + i + 1);
        let body_end = body_start.and_then(|b| t[b..].find("```").map(|i| b + i));
        if let (Some(b), Some(e)) = (body_start, body_end) {
            if let Ok(r) = serde_json::from_str::<StructuredResponse>(t[b..e].trim()) {
                return Ok(r);
            }
        }
    }
    if let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) {
        if a < b {
            if let Ok(r) = serde_json::from_str::<StructuredResponse>(&t[a..=b]) {
                return Ok(r);
            }
        }
    }
    Err("response did not contain the expected JSON ({\"files\": [...], \"notes\": ...})".into())
}

fn apply_changes(root: &Path, resp: &StructuredResponse) -> Result<Vec<String>, String> {
    // Validate every path before writing anything.
    for f in &resp.files {
        let rel = Path::new(&f.path);
        if rel.is_absolute() || f.path.split(['/', '\\']).any(|c| c == "..") {
            return Err(format!("refusing suspicious path in response: {}", f.path));
        }
    }
    let mut applied = Vec::new();
    for f in &resp.files {
        let dst = root.join(f.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&dst, &f.content).map_err(|e| format!("cannot write {}: {e}", dst.display()))?;
        applied.push(f.path.clone());
    }
    Ok(applied)
}

const TAIL_CHARS: usize = 400;

fn tail(s: &str) -> String {
    if s.len() <= TAIL_CHARS {
        s.to_string()
    } else {
        let cut = s.len() - TAIL_CHARS;
        let start = (cut..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(cut);
        format!("…{}", &s[start..])
    }
}

/// Spawn `claude -p <prompt>` in `cwd` as a tracked background job.
pub fn start_job(
    state: &JobState,
    label: String,
    cwd: String,
    prompt: String,
    model: Option<String>,
) -> Result<u64, String> {
    let dir = PathBuf::from(&cwd);
    if !dir.is_dir() {
        return Err(format!("{cwd} is not a directory"));
    }

    let model = model.filter(|m| !m.trim().is_empty());
    // Read-only tools: edits come back as a structured response the app
    // applies itself (the CLI can't write inside ~/.claude anyway).
    let mut args: Vec<&str> = vec!["-p", &prompt, "--allowedTools", "Read,Glob,Grep"];
    if let Some(m) = model.as_deref() {
        args.push("--model");
        args.push(m);
    }
    let mut child = Command::new("claude")
        .current_dir(&dir)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch claude CLI: {e}"))?;

    let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    let output = Arc::new(Mutex::new(String::new()));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));

    let apply_root = cwd.clone();
    let info = JobInfo {
        id,
        label,
        cwd,
        prompt,
        model,
        applied_files: Vec::new(),
        notes: None,
        status: JobStatus::Running,
        output_tail: String::new(),
        started_at: now_secs(),
        finished_at: None,
    };
    state.jobs.lock().unwrap().insert(
        id,
        Job { info, output: output.clone(), child: Some(child.clone()) },
    );

    // Drain stderr on its own thread so the child never blocks on a full pipe.
    let err_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    if let Some(mut se) = stderr {
        let err_buf = err_buf.clone();
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = se.read_to_string(&mut buf);
            *err_buf.lock().unwrap() = buf;
        });
    }

    // Reader/waiter thread: stream stdout, then reap the child and set status.
    let manager = state.clone();
    std::thread::spawn(move || {
        if let Some(mut so) = stdout {
            let mut chunk = [0u8; 4096];
            loop {
                match so.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&chunk[..n]).to_string();
                        output.lock().unwrap().push_str(&text);
                    }
                    Err(_) => break,
                }
            }
        }
        let status = child.lock().unwrap().wait();
        let err_text = err_buf.lock().unwrap().clone();
        if !err_text.trim().is_empty() {
            output.lock().unwrap().push_str(&format!("\n[stderr]\n{err_text}"));
        }

        // On success, parse the structured response and apply the file
        // changes ourselves. Failures leave every file untouched (paths are
        // validated before the first write) and keep the raw output.
        let mut applied: Vec<String> = Vec::new();
        let mut notes: Option<String> = None;
        let mut apply_failed = false;
        if matches!(&status, Ok(s) if s.success()) {
            let text = output.lock().unwrap().clone();
            match parse_response(&text).and_then(|resp| {
                notes = resp.notes.clone();
                apply_changes(Path::new(&apply_root), &resp)
            }) {
                Ok(files) => applied = files,
                Err(e) => {
                    apply_failed = true;
                    output.lock().unwrap().push_str(&format!("\n[apply error] {e}"));
                }
            }
        }

        let mut jobs = manager.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if job.info.status == JobStatus::Running {
                job.info.status = match status {
                    Ok(s) if s.success() && !apply_failed => JobStatus::Done,
                    _ => JobStatus::Failed,
                };
            }
            job.info.applied_files = applied;
            job.info.notes = notes;
            job.info.finished_at = Some(now_secs());
            job.child = None;
        }
    });

    Ok(id)
}

pub fn list_jobs(state: &JobState) -> Vec<JobInfo> {
    let jobs = state.jobs.lock().unwrap();
    let mut infos: Vec<JobInfo> = jobs
        .values()
        .map(|j| {
            let mut info = j.info.clone();
            info.output_tail = tail(&j.output.lock().unwrap());
            info
        })
        .collect();
    infos.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
    infos
}

pub fn job_output(state: &JobState, id: u64) -> Result<String, String> {
    let jobs = state.jobs.lock().unwrap();
    jobs.get(&id)
        .map(|j| j.output.lock().unwrap().clone())
        .ok_or_else(|| format!("no job {id}"))
}

pub fn cancel_job(state: &JobState, id: u64) -> Result<(), String> {
    let mut jobs = state.jobs.lock().unwrap();
    let job = jobs.get_mut(&id).ok_or_else(|| format!("no job {id}"))?;
    if let Some(child) = &job.child {
        let _ = child.lock().unwrap().kill();
        job.info.status = JobStatus::Cancelled;
        job.info.finished_at = Some(now_secs());
    }
    Ok(())
}

pub fn clear_finished(state: &JobState) {
    let mut jobs = state.jobs.lock().unwrap();
    jobs.retain(|_, j| j.info.status == JobStatus::Running);
}
