import type { InstrCreateKind } from "./api";
import type { InstrFile, InstrGroup, InstrOverview, StartupEntry } from "./types";

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** A deliberately rough estimate (≈4 bytes per token) — always label it so. */
export function roughTokens(bytes: number): string {
  const t = Math.round(bytes / 4);
  return t >= 1000 ? `~${(t / 1000).toFixed(1)}k tokens` : `~${t} tokens`;
}

export function startupTotal(entries: StartupEntry[]): number {
  return entries.reduce((n, e) => n + e.loaded_bytes, 0);
}

export interface Badge {
  text: string;
  tone: "" | "warn" | "danger" | "ok" | "info";
}

/** Compact badges for a sidebar row, most important first. */
export function fileBadges(f: InstrFile): Badge[] {
  const out: Badge[] = [];
  if (f.excluded) out.push({ text: "excluded", tone: "danger" });
  else if (f.loads === "on-demand") out.push({ text: f.paths.length ? "paths" : "on demand", tone: "" });
  else if (f.loads === "imported") out.push({ text: "imported", tone: "ok" });
  else if (f.loads === "never" && f.kind !== "other-agent") out.push({ text: "not loaded", tone: "danger" });
  if (f.kind === "local") out.push({ text: "local", tone: "info" });
  if (f.kind === "other-agent" && f.agent) out.push({ text: f.agent, tone: "" });
  if (f.warnings.length) out.push({ text: "⚠", tone: "warn" });
  if (!f.editable) out.push({ text: "read-only", tone: "warn" });
  return out;
}

/** One sentence on when Claude Code reads this file. */
export function loadsText(f: InstrFile): string {
  if (f.excluded) return "Not loaded — matched by claudeMdExcludes.";
  switch (f.loads) {
    case "startup":
      return "Loaded at the start of every session in scope.";
    case "on-demand":
      if (f.paths.length) return `Loaded when Claude reads a file matching ${f.paths.join(", ")}.`;
      return "Loaded when Claude reads files in this folder.";
    case "imported":
      return `Claude Code doesn't read this file by name, but ${f.imported_by.join(", ")} imports it, so it loads at startup.`;
    case "never":
      if (f.kind === "other-agent")
        return "Claude Code doesn't read this file. Add @" + f.label + " to a CLAUDE.md to share it.";
      return "Not loaded.";
  }
}

export type Section = "instructions" | "rules" | "nested" | "other";

export function sectionOf(f: InstrFile): Section {
  switch (f.kind) {
    case "rule":
      return f.label.startsWith(".claude/rules/") || f.label.startsWith("rules/") ? "rules" : "nested";
    case "nested":
      return "nested";
    case "other-agent":
      return "other";
    default:
      return "instructions";
  }
}

export const SECTION_TITLE: Record<Section, string> = {
  instructions: "",
  rules: "rules",
  nested: "subfolders (on demand)",
  other: "other agents",
};

export function findFile(ov: InstrOverview | null, path: string): { group: InstrGroup; file: InstrFile } | null {
  for (const group of ov?.groups ?? []) {
    const file = group.files.find((f) => f.path === path);
    if (file) return { group, file };
  }
  return null;
}

/** Case-insensitive filter over group and file labels. */
export function filterGroups(groups: InstrGroup[], query: string): InstrGroup[] {
  const q = query.trim().toLowerCase();
  if (!q) return groups;
  return groups
    .map((g) =>
      g.label.toLowerCase().includes(q)
        ? g
        : { ...g, files: g.files.filter((f) => f.label.toLowerCase().includes(q)) },
    )
    .filter((g) => g.label.toLowerCase().includes(q) || g.files.length > 0);
}

export interface CreateOption {
  kind: InstrCreateKind;
  label: string;
  /** Relative path the file will have (rules: without the name). */
  path: string;
}

export function createOptions(project: boolean): CreateOption[] {
  return project
    ? [
        { kind: "claude", label: "CLAUDE.md — shared project instructions", path: "CLAUDE.md" },
        { kind: "dot-claude", label: ".claude/CLAUDE.md — same, kept in .claude", path: ".claude/CLAUDE.md" },
        { kind: "local", label: "CLAUDE.local.md — personal, not committed", path: "CLAUDE.local.md" },
        { kind: "rule", label: "Rule — .claude/rules/<name>.md", path: ".claude/rules/" },
        { kind: "agents", label: "AGENTS.md — for other coding agents", path: "AGENTS.md" },
        { kind: "gemini", label: "GEMINI.md — for Gemini CLI", path: "GEMINI.md" },
      ]
    : [
        { kind: "claude", label: "~/.claude/CLAUDE.md — your instructions for every project", path: "CLAUDE.md" },
        { kind: "rule", label: "Rule — ~/.claude/rules/<name>.md", path: "rules/" },
      ];
}

/** The existing file a create option would collide with, if any. */
export function existingFor(group: InstrGroup | undefined, option: CreateOption): InstrFile | undefined {
  if (!group || option.kind === "rule") return undefined;
  return group.files.find((f) => f.label === option.path);
}
