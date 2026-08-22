mod ai;
mod discovery;
mod files;
mod git;
mod install;
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
    sync::snapshot(&settings)
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
fn ai_start_job(
    state: tauri::State<ai::JobState>,
    label: String,
    cwd: String,
    prompt: String,
) -> Result<u64, String> {
    ai::start_job(&state, label, cwd, prompt)
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
            ai_start_job,
            ai_list_jobs,
            ai_job_output,
            ai_cancel_job,
            ai_clear_finished,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
