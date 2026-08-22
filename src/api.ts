import { invoke } from "@tauri-apps/api/core";
import type { JobInfo, Settings, SkillGroup, SyncStatus } from "./types";

export const discoverSkills = () => invoke<SkillGroup[]>("discover_skills");

export const readSkillFile = (path: string) =>
  invoke<string>("read_skill_file", { path });

export const writeSkillFile = (path: string, content: string) =>
  invoke<void>("write_skill_file", { path, content });

export const createSkill = (root: string, name: string, description: string) =>
  invoke<string>("create_skill", { root, name, description });

export const deleteSkill = (dir: string) => invoke<void>("delete_skill", { dir });

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (newSettings: Settings) =>
  invoke<void>("save_settings", { newSettings });

export const syncStatus = () => invoke<SyncStatus>("sync_status");
export const syncInit = () => invoke<string>("sync_init");
export const syncSnapshot = () => invoke<string>("sync_snapshot");
export const syncPush = () => invoke<string>("sync_push");
export const syncFetch = () => invoke<string>("sync_fetch");
export const syncPull = () => invoke<string>("sync_pull");

export const aiStartJob = (label: string, cwd: string, prompt: string) =>
  invoke<number>("ai_start_job", { label, cwd, prompt });

export const aiListJobs = () => invoke<JobInfo[]>("ai_list_jobs");

export const aiJobOutput = (id: number) => invoke<string>("ai_job_output", { id });

export const aiCancelJob = (id: number) => invoke<void>("ai_cancel_job", { id });

export const aiClearFinished = () => invoke<void>("ai_clear_finished");

export const installSkill = (
  repoUrl: string,
  subpath: string,
  name: string,
  overwrite: boolean,
) => invoke<string>("install_skill", { repoUrl, subpath, name, overwrite });
