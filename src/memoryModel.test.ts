import { expect, test } from "bun:test";
import {
  age,
  entryAge,
  filterStores,
  findEntry,
  findIndex,
  matchesFilter,
  storeSummary,
  typeCounts,
} from "./memoryModel";
import type { MemoryEntry, MemoryStore } from "./types";

const entry = (over: Partial<MemoryEntry>): MemoryEntry => ({
  path: "/m/a.md",
  rel: "a.md",
  name: "a",
  description: null,
  kind: "feedback",
  modified: null,
  mtime: 1_700_000_000,
  bytes: 10,
  lines: 2,
  in_index: true,
  has_frontmatter: true,
  ...over,
});

const store = (over: Partial<MemoryStore>): MemoryStore => ({
  key: "s",
  kind: "project",
  label: "app",
  dir: "/m",
  exists: true,
  project_dir: "/p",
  enabled: true,
  enabled_source: "default",
  custom_dir: false,
  agent: null,
  index: { path: "/m/MEMORY.md", exists: true, bytes: 10, lines: 2, loaded_bytes: 10, truncated: false, links: [] },
  entries: [],
  warnings: [],
  ...over,
});

test("age buckets", () => {
  const now = 1_700_000_000_000;
  expect(age(0, now)).toBe("");
  expect(age(now / 1000 - 30, now)).toBe("just now");
  expect(age(now / 1000 - 600, now)).toBe("10m");
  expect(age(now / 1000 - 7200, now)).toBe("2h");
  expect(age(now / 1000 - 3 * 86400, now)).toBe("3d");
  expect(age(now / 1000 - 90 * 86400, now)).toBe("3mo");
  expect(age(now / 1000 - 800 * 86400, now)).toBe("2y");
});

test("entry age prefers the modified field, falling back to mtime", () => {
  const now = Date.parse("2026-01-10T00:00:00Z");
  expect(entryAge(entry({ modified: "2026-01-08T00:00:00Z" }), now)).toBe("2d");
  expect(entryAge(entry({ modified: "nonsense", mtime: Math.round(now / 1000) - 3600 }), now)).toBe("1h");
  expect(entryAge(entry({ modified: null, mtime: Math.round(now / 1000) - 86400 }), now)).toBe("1d");
});

test("store summary explains what loads", () => {
  expect(storeSummary(store({ entries: [entry({})] }))).toContain("MEMORY.md loads at session start");
  expect(storeSummary(store({ entries: [entry({})], enabled: false, enabled_source: "user settings.json" }))).toContain(
    "auto memory is off (user settings.json)",
  );
  expect(
    storeSummary(store({ index: { ...store({}).index, exists: false }, entries: [entry({}), entry({})] })),
  ).toBe("2 notes, but there's no MEMORY.md index.");
  expect(storeSummary(store({ exists: false }))).toBe("This folder doesn't exist yet.");
  expect(
    storeSummary(store({ exists: false, label: "explorer", agent: { name: "explorer", scope: "user", declared: true, file: "/a.md" } })),
  ).toContain("explorer has memory enabled but hasn't saved anything");
});

test("filtering by text, type, and index state", () => {
  const notes = [
    entry({ path: "1", name: "deploy needs vpn", kind: "project" }),
    entry({ path: "2", name: "commit style", kind: "feedback", in_index: false }),
    entry({ path: "3", name: "dashboards", kind: "reference", description: "grafana" }),
  ];
  const stores = [store({ entries: notes }), store({ key: "b", label: "other", entries: [entry({ path: "4", name: "x" })] })];
  const f = { query: "", types: [], unindexed: false };

  expect(filterStores(stores, f)).toBe(stores);
  expect(matchesFilter(notes[2], { ...f, query: "GRAFANA" })).toBe(true);
  expect(filterStores(stores, { ...f, query: "deploy" })[0].entries.map((e) => e.path)).toEqual(["1"]);
  expect(filterStores(stores, { ...f, query: "deploy" }).length).toBe(1);
  expect(filterStores(stores, { ...f, types: ["feedback"] })[0].entries.map((e) => e.path)).toEqual(["2"]);
  expect(filterStores(stores, { ...f, unindexed: true })[0].entries.map((e) => e.path)).toEqual(["2"]);
  // A store whose name matches is kept even with no matching notes.
  expect(filterStores(stores, { ...f, query: "other" }).map((s) => s.label)).toEqual(["other"]);
});

test("type counts include untyped notes", () => {
  const stores = [
    store({ entries: [entry({ kind: "user" }), entry({ kind: "user" }), entry({ kind: null })] }),
    store({ key: "b", entries: [entry({ kind: "project" })] }),
  ];
  expect(typeCounts(stores)).toEqual({ user: 2, untyped: 1, project: 1 });
});

test("lookups by path", () => {
  const stores = [store({ entries: [entry({ path: "/m/a.md" })] })];
  expect(findEntry(stores, "/m/a.md")?.store.label).toBe("app");
  expect(findEntry(stores, "/m/nope.md")).toBeNull();
  expect(findIndex(stores, "/m/MEMORY.md")?.label).toBe("app");
  expect(findIndex(stores, "/m/a.md")).toBeNull();
});
