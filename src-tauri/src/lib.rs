mod ai;
mod discovery;
mod files;
mod git;
mod hooks;
mod install;
mod instructions;
mod overrides;
mod paths;
mod settings;
mod sync;

use discovery::SkillGroup;
use settings::Settings;
use sync::SyncStatus;

#[tauri::command]
fn discover_skills(app: tauri::AppHandle) -> Result<Vec<SkillGroup>, String> {
    let settings = settings::load(&app);
    discovery::discover(&settings)
}

#[tauri::command]
fn read_skill_file(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let settings = settings::load(&app);
    files::read_text(&path, &settings)
}

#[tauri::command]
fn write_skill_file(app: tauri::AppHandle, path: String, content: String) -> Result<(), String> {
    let settings = settings::load(&app);
    files::write_text(&path, &content, &settings)
}

#[tauri::command]
fn create_skill(
    app: tauri::AppHandle,
    root: String,
    name: String,
    description: String,
) -> Result<String, String> {
    let settings = settings::load(&app);
    files::create_skill(&root, &name, &description, &settings)
}

#[tauri::command]
fn delete_skill(app: tauri::AppHandle, dir: String) -> Result<(), String> {
    let settings = settings::load(&app);
    files::delete_skill(&dir, &settings)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, new_settings: Settings) -> Result<(), String> {
    settings::save(&app, &new_settings)
}

#[tauri::command]
fn sync_status(app: tauri::AppHandle) -> Result<SyncStatus, String> {
    let settings = settings::load(&app);
    sync::status(&settings)
}

#[tauri::command]
fn sync_init(app: tauri::AppHandle) -> Result<String, String> {
    let settings = settings::load(&app);
    sync::init(&settings)
}

#[tauri::command]
fn sync_snapshot(app: tauri::AppHandle) -> Result<String, String> {
    let settings = settings::load(&app);
    sync::snapshot(&settings, &settings::disabled_hooks_file(&app)?)
}

#[tauri::command]
fn sync_push(app: tauri::AppHandle) -> Result<String, String> {
    let settings = settings::load(&app);
    sync::push(&settings)
}

#[tauri::command]
fn sync_fetch(app: tauri::AppHandle) -> Result<String, String> {
    let settings = settings::load(&app);
    sync::fetch(&settings)
}

#[tauri::command]
fn sync_pull(app: tauri::AppHandle) -> Result<String, String> {
    let settings = settings::load(&app);
    sync::pull(&settings)
}

#[tauri::command]
fn install_skill(
    repo_url: String,
    subpath: String,
    name: String,
    overwrite: bool,
) -> Result<String, String> {
    install::install_skill(&repo_url, &subpath, &name, overwrite)
}

#[tauri::command]
fn set_skill_enabled(skill_dir: String, enabled: bool) -> Result<String, String> {
    overrides::set_skill_enabled(&skill_dir, enabled)
}

#[tauri::command]
fn hooks_overview(app: tauri::AppHandle) -> Result<hooks::HooksOverview, String> {
    let settings = settings::load(&app);
    hooks::overview(&settings, Some(&settings::disabled_hooks_file(&app)?))
}

#[tauri::command]
fn hooks_set(file: String, expected_hash: String, hooks: serde_json::Value) -> Result<String, String> {
    hooks::set_hooks(std::path::Path::new(&file), &expected_hash, &hooks)
}

#[tauri::command]
fn hooks_set_disable_all(file: String, expected_hash: String, disabled: bool) -> Result<String, String> {
    hooks::set_disable_all(std::path::Path::new(&file), &expected_hash, disabled)
}

#[tauri::command]
fn hooks_disable(
    app: tauri::AppHandle,
    file: String,
    expected_hash: String,
    event: String,
    group: usize,
    handler: usize,
) -> Result<(), String> {
    hooks::disable_hook(
        std::path::Path::new(&file),
        &expected_hash,
        &event,
        group,
        handler,
        &settings::disabled_hooks_file(&app)?,
    )
}

#[tauri::command]
fn hooks_enable(app: tauri::AppHandle, id: String) -> Result<(), String> {
    hooks::enable_hook(&id, &settings::disabled_hooks_file(&app)?)
}

#[tauri::command]
fn hooks_delete_parked(app: tauri::AppHandle, id: String) -> Result<(), String> {
    hooks::delete_parked(&id, &settings::disabled_hooks_file(&app)?)
}

/// Runs a hook command; async so a slow hook never blocks the UI thread.
#[tauri::command]
async fn hooks_test(request: hooks::TestRequest) -> Result<hooks::TestResult, String> {
    tauri::async_runtime::spawn_blocking(move || hooks::run_test(request))
        .await
        .map_err(|e| format!("test runner crashed: {e}"))?
}

/// Scans every registered project, so it runs off the UI thread.
#[tauri::command]
async fn instructions_overview(force: bool) -> Result<instructions::InstrOverview, String> {
    tauri::async_runtime::spawn_blocking(move || instructions::overview(force))
        .await
        .map_err(|e| format!("instructions scan crashed: {e}"))?
}

#[derive(serde::Serialize)]
struct Created {
    path: String,
    note: Option<String>,
}

#[tauri::command]
fn instructions_create(
    project: Option<String>,
    kind: String,
    name: Option<String>,
    paths: Vec<String>,
    gitignore: bool,
) -> Result<Created, String> {
    let (path, note) =
        instructions::create(project.as_deref(), &kind, name.as_deref(), &paths, gitignore)?;
    Ok(Created { path, note })
}

#[tauri::command]
fn instructions_delete(path: String) -> Result<(), String> {
    instructions::delete(&path)
}

#[tauri::command]
fn instructions_set_auto_memory(project: Option<String>, enabled: bool) -> Result<String, String> {
    instructions::set_auto_memory(project.as_deref(), enabled)
}

#[tauri::command]
fn ai_start_job(
    state: tauri::State<ai::JobState>,
    label: String,
    cwd: String,
    prompt: String,
    model: Option<String>,
) -> Result<u64, String> {
    ai::start_job(&state, label, cwd, prompt, model)
}

#[tauri::command]
fn ai_list_jobs(state: tauri::State<ai::JobState>) -> Vec<ai::JobInfo> {
    ai::list_jobs(&state)
}

#[tauri::command]
fn ai_job_output(state: tauri::State<ai::JobState>, id: u64) -> Result<String, String> {
    ai::job_output(&state, id)
}

#[tauri::command]
fn ai_cancel_job(state: tauri::State<ai::JobState>, id: u64) -> Result<(), String> {
    ai::cancel_job(&state, id)
}

#[tauri::command]
fn ai_clear_finished(state: tauri::State<ai::JobState>) {
    ai::clear_finished(&state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(ai::new_state())
        .invoke_handler(tauri::generate_handler![
            discover_skills,
            read_skill_file,
            write_skill_file,
            create_skill,
            delete_skill,
            get_settings,
            save_settings,
            sync_status,
            sync_init,
            sync_snapshot,
            sync_push,
            sync_fetch,
            sync_pull,
            install_skill,
            set_skill_enabled,
            hooks_overview,
            hooks_set,
            hooks_set_disable_all,
            hooks_disable,
            hooks_enable,
            hooks_delete_parked,
            hooks_test,
            instructions_overview,
            instructions_create,
            instructions_delete,
            instructions_set_auto_memory,
            ai_start_job,
            ai_list_jobs,
            ai_job_output,
            ai_cancel_job,
            ai_clear_finished,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
