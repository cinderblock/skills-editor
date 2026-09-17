import { expect, test } from "bun:test";
import {
  createOptions,
  existingFor,
  fileBadges,
  filterGroups,
  formatBytes,
  loadsText,
  roughTokens,
  sectionOf,
  startupTotal,
} from "./instructionsModel";
import type { InstrFile, InstrGroup } from "./types";

const file = (over: Partial<InstrFile>): InstrFile => ({
  path: "/p/CLAUDE.md",
  label: "CLAUDE.md",
  kind: "claude",
  loads: "startup",
  editable: true,
  bytes: 10,
  lines: 1,
  excluded: false,
  paths: [],
  agent: null,
  imported_by: [],
  applies_to: [],
  imports: [],
  memory: null,
  warnings: [],
  ...over,
});

const group = (label: string, files: InstrFile[]): InstrGroup => ({
  key: label,
  kind: "project",
  label,
  detail: "",
  project_dir: "/p",
  files,
  startup: [],
  auto_memory: null,
  notes: [],
});

test("sizes and rough token estimates", () => {
  expect(formatBytes(512)).toBe("512 B");
  expect(formatBytes(2048)).toBe("2.0 KB");
  expect(formatBytes(40 * 1024)).toBe("40 KB");
  expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
  expect(roughTokens(400)).toBe("~100 tokens");
  expect(roughTokens(17069)).toBe("~4.3k tokens");
  expect(startupTotal([
    { path: "a", label: "a", depth: 0, bytes: 900, loaded_bytes: 100, lines: 1, note: null },
    { path: "b", label: "b", depth: 1, bytes: 50, loaded_bytes: 50, lines: 1, note: null },
  ])).toBe(150);
});

test("badges put exclusion and load state first", () => {
  expect(fileBadges(file({ excluded: true, loads: "never", kind: "local" })).map((b) => b.text)).toEqual(["excluded", "local"]);
  expect(fileBadges(file({ kind: "rule", loads: "on-demand", paths: ["src/**"] }))[0].text).toBe("paths");
  expect(fileBadges(file({ kind: "other-agent", loads: "never", agent: "Gemini CLI" })).map((b) => b.text)).toEqual(["Gemini CLI"]);
  expect(fileBadges(file({ kind: "other-agent", loads: "imported", agent: "Codex" }))[0].text).toBe("imported");
  expect(fileBadges(file({ warnings: ["x"], editable: false })).map((b) => b.text)).toEqual(["⚠", "read-only"]);
  expect(fileBadges(file({ kind: "memory-topic", loads: "on-demand", memory: { name: "n", description: null, kind: "feedback" } })).map((b) => b.text)).toEqual(["on demand", "feedback"]);
});

test("load explanations", () => {
  expect(loadsText(file({ kind: "rule", loads: "on-demand", paths: ["a/**", "b/*"] }))).toBe(
    "Loaded when Claude reads a file matching a/**, b/*.",
  );
  expect(loadsText(file({ kind: "other-agent", loads: "imported", imported_by: ["CLAUDE.md"] }))).toContain("CLAUDE.md imports it");
  expect(loadsText(file({ kind: "other-agent", loads: "never", label: "AGENTS.md" }))).toContain("@AGENTS.md");
  expect(loadsText(file({ excluded: true, loads: "never" }))).toContain("claudeMdExcludes");
  expect(loadsText(file({ kind: "memory-index" }))).toContain("200 lines");
});

test("sections", () => {
  expect(sectionOf(file({ kind: "rule", label: ".claude/rules/a.md" }))).toBe("rules");
  expect(sectionOf(file({ kind: "rule", label: "rules/a.md" }))).toBe("rules");
  expect(sectionOf(file({ kind: "rule", label: "pkg/.claude/rules/a.md" }))).toBe("nested");
  expect(sectionOf(file({ kind: "nested" }))).toBe("nested");
  expect(sectionOf(file({ kind: "memory-topic" }))).toBe("memory");
  expect(sectionOf(file({ kind: "other-agent" }))).toBe("other");
  expect(sectionOf(file({ kind: "local" }))).toBe("instructions");
});

test("filter keeps matching groups whole and trims others", () => {
  const groups = [
    group("app", [file({ label: "CLAUDE.md" }), file({ label: "auto memory/x.md", memory: { name: "deploy notes", description: null, kind: null } })]),
    group("deploy-tool", [file({ label: "CLAUDE.md" })]),
    group("other", [file({ label: "CLAUDE.md" })]),
  ];
  const out = filterGroups(groups, "DEPLOY");
  expect(out.map((g) => g.label)).toEqual(["app", "deploy-tool"]);
  expect(out[0].files.map((f) => f.label)).toEqual(["auto memory/x.md"]);
  expect(out[1].files.length).toBe(1);
  expect(filterGroups(groups, "  ")).toBe(groups);
});

test("create options and collisions", () => {
  expect(createOptions(false).map((o) => o.kind)).toEqual(["claude", "rule"]);
  const project = createOptions(true);
  expect(project.map((o) => o.kind)).toEqual(["claude", "dot-claude", "local", "rule", "agents", "gemini"]);
  const g = group("app", [file({ label: "CLAUDE.md" })]);
  expect(existingFor(g, project[0])?.label).toBe("CLAUDE.md");
  expect(existingFor(g, project[2])).toBeUndefined();
  expect(existingFor(g, project[3])).toBeUndefined();
  expect(existingFor(undefined, project[0])).toBeUndefined();
});
