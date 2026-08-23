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
  /** The skill this file belongs to. */
  skill: Skill;
  group: SkillGroup;
}
