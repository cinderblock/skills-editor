use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
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
pub fn start_job(state: &JobState, label: String, cwd: String, prompt: String) -> Result<u64, String> {
    let dir = PathBuf::from(&cwd);
    if !dir.is_dir() {
        return Err(format!("{cwd} is not a directory"));
    }

    let mut child = Command::new("claude")
        .current_dir(&dir)
        .args(["-p", &prompt, "--permission-mode", "acceptEdits"])
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

    let info = JobInfo {
        id,
        label,
        cwd,
        prompt,
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
        let mut jobs = manager.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if job.info.status == JobStatus::Running {
                job.info.status = match status {
                    Ok(s) if s.success() => JobStatus::Done,
                    _ => JobStatus::Failed,
                };
            }
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
