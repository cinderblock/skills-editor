export interface Skill {
  id: string;
  name: string;
  description: string;
  dir: string;
  skill_md: string;
  files: string[];
  single_file: boolean;
  editable: boolean;
  disabled: boolean;
}

export type GroupKind = "user" | "project" | "plugin" | "extra";

export interface SkillGroup {
  key: string;
  kind: GroupKind;
  label: string;
  detail: string;
  skills: Skill[];
}

export interface Settings {
  repo_path: string | null;
  remote_url: string | null;
  extra_roots: string[];
}

export interface SyncStatus {
  repo_path: string;
  initialized: boolean;
  hostname: string;
  branch: string | null;
  on_host_branch: boolean;
  dirty: boolean;
  last_commit: string | null;
  branches: string[];
  remote: string | null;
}

export type JobStatus = "running" | "done" | "failed" | "cancelled";

export interface JobInfo {
  id: number;
  label: string;
  cwd: string;
  prompt: string;
  model: string | null;
  applied_files: string[];
  notes: string | null;
  status: JobStatus;
  output_tail: string;
  started_at: number;
  finished_at: number | null;
}

export interface OpenFile {
  /** Absolute path of the file being edited. */
  path: string;
  /**
   * The skill this file belongs to. Hook scripts get a stand-in whose `dir`
   * is the script's folder (AI edits run there).
   */
  skill: Skill;
  group: SkillGroup;
  /** Defaults to "skill". Scripts and instruction files hide skill-only actions. */
  kind?: "skill" | "script" | "instruction";
}

// ---- instructions & memory ----

export type InstrFileKind =
  | "claude"
  | "local"
  | "rule"
  | "nested"
  | "memory-index"
  | "memory-topic"
  | "other-agent";

export type InstrLoads = "startup" | "on-demand" | "imported" | "never";

export interface InstrImport {
  raw: string;
  path: string;
  exists: boolean;
  external: boolean;
}

export interface InstrFile {
  path: string;
  label: string;
  kind: InstrFileKind;
  loads: InstrLoads;
  editable: boolean;
  bytes: number;
  lines: number;
  excluded: boolean;
  paths: string[];
  agent: string | null;
  imported_by: string[];
  applies_to: string[];
  imports: InstrImport[];
  memory: { name: string | null; description: string | null; kind: string | null } | null;
  warnings: string[];
}

export interface StartupEntry {
  path: string;
  label: string;
  depth: number;
  bytes: number;
  loaded_bytes: number;
  lines: number;
  note: string | null;
}

export interface AutoMemoryState {
  dir: string;
  exists: boolean;
  enabled: boolean;
  source: string;
  custom_dir: boolean;
}

export interface InstrGroup {
  key: string;
  kind: "managed" | "user" | "project" | "parents" | "memory-other";
  label: string;
  detail: string;
  project_dir: string | null;
  files: InstrFile[];
  startup: StartupEntry[];
  auto_memory: AutoMemoryState | null;
  notes: string[];
}

export interface InstrOverview {
  groups: InstrGroup[];
  projects: { dir: string; label: string }[];
}

/** What the Memory view's main pane shows. */
export type InstrSelection =
  | { kind: "file"; path: string }
  | { kind: "startup"; group: string };

// ---- hooks ----

export interface ScriptRef {
  event: string;
  group: number;
  handler: number;
  path: string;
  exists: boolean;
}

export interface ParkedHook {
  id: string;
  file: string;
  event: string;
  matcher: string | null;
  group_extra: Record<string, unknown>;
  handler: Record<string, unknown>;
  disabled_at: number;
}

export type HookSourceKind =
  | "user"
  | "project"
  | "local"
  | "managed"
  | "plugin"
  | "skill"
  | "agent"
  | "orphaned";

export interface HookSource {
  file: string;
  file_label: string;
  kind: HookSourceKind;
  editable: boolean;
  exists: boolean;
  hash: string;
  hooks: Record<string, unknown>;
  disable_all_hooks: boolean;
  inactive_reason: string | null;
  project_dir: string | null;
  plugin_root: string | null;
  scripts: ScriptRef[];
  parked: ParkedHook[];
  parse_error: string | null;
}

export interface ScriptFile {
  path: string;
  name: string;
  referenced: boolean;
  editable: boolean;
}

export interface HookGroup {
  key: string;
  kind: "user" | "project" | "managed" | "plugin" | "frontmatter" | "orphaned";
  label: string;
  detail: string;
  sources: HookSource[];
  scripts: ScriptFile[];
}

export interface HookTarget {
  file: string;
  label: string;
  project_dir: string | null;
}

export interface HooksOverview {
  groups: HookGroup[];
  targets: HookTarget[];
  events: string[];
}

/** What the hook editor is showing. */
export type HookSelection =
  | { kind: "handler"; file: string; event: string; group: number; handler: number }
  | { kind: "parked"; id: string }
  | { kind: "new"; file: string | null };

export interface HookTestRequest {
  command: string;
  args: string[] | null;
  shell: string | null;
  timeout_secs: number | null;
  project_dir: string | null;
  plugin_root: string | null;
  stdin: string;
}

export interface HookTestResult {
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
  runner: string;
}
