import { invoke } from "@tauri-apps/api/core";
import type {
  HookTestRequest,
  HookTestResult,
  HooksOverview,
  InstrOverview,
  JobInfo,
  MemoryOverview,
  Settings,
  SkillGroup,
  SyncStatus,
} from "./types";

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

export const setSkillEnabled = (skillDir: string, enabled: boolean) =>
  invoke<string>("set_skill_enabled", { skillDir, enabled });

// ---- hooks ----

export const hooksOverview = () => invoke<HooksOverview>("hooks_overview");

/** Replace a settings file's `hooks` block. Returns the file's new hash. */
export const hooksSet = (file: string, expectedHash: string, hooks: unknown) =>
  invoke<string>("hooks_set", { file, expectedHash, hooks });

export const hooksSetDisableAll = (file: string, expectedHash: string, disabled: boolean) =>
  invoke<string>("hooks_set_disable_all", { file, expectedHash, disabled });

export const hooksDisable = (
  file: string,
  expectedHash: string,
  event: string,
  group: number,
  handler: number,
) => invoke<void>("hooks_disable", { file, expectedHash, event, group, handler });

export const hooksEnable = (id: string) => invoke<void>("hooks_enable", { id });

export const hooksDeleteParked = (id: string) => invoke<void>("hooks_delete_parked", { id });

export const hooksTest = (request: HookTestRequest) =>
  invoke<HookTestResult>("hooks_test", { request });

// ---- instructions & memory ----

/** `force` rescans nested CLAUDE.md files instead of reusing recent results. */
export const instructionsOverview = (force: boolean) =>
  invoke<InstrOverview>("instructions_overview", { force });

export type InstrCreateKind = "claude" | "dot-claude" | "local" | "rule" | "agents" | "gemini";

export const instructionsCreate = (args: {
  project: string | null;
  kind: InstrCreateKind;
  name: string | null;
  paths: string[];
  gitignore: boolean;
}) => invoke<{ path: string; note: string | null }>("instructions_create", args);

export const instructionsDelete = (path: string) => invoke<void>("instructions_delete", { path });

export const instructionsSetAutoMemory = (project: string | null, enabled: boolean) =>
  invoke<string>("instructions_set_auto_memory", { project, enabled });

// ---- memory ----

export const memoryOverview = () => invoke<MemoryOverview>("memory_overview");

export const memoryCreate = (args: {
  dir: string;
  name: string;
  description: string;
  kind: string;
  body: string;
  index: boolean;
}) => invoke<string>("memory_create", args);

export const memoryDelete = (path: string, fromIndex: boolean) =>
  invoke<string>("memory_delete", { path, fromIndex });

export const memoryIndexEntry = (path: string) => invoke<string>("memory_index_entry", { path });

/** Backend errors for stale writes start with this marker. */
export const isConflict = (e: unknown) => String(e).includes("CONFLICT:");

export const aiStartJob = (
  label: string,
  cwd: string,
  prompt: string,
  model: string | null,
) => invoke<number>("ai_start_job", { label, cwd, prompt, model });

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
