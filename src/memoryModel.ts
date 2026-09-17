import type { MemoryEntry, MemoryStore } from "./types";

export const TYPE_HELP: Record<string, string> = {
  user: "who you are — role, expertise, preferences",
  feedback: "corrections you gave Claude, and approaches you confirmed",
  project: "ongoing work and decisions Claude can't read from the code",
  reference: "where to find things outside the project",
};

export const TYPE_TONE: Record<string, string> = {
  user: "info",
  feedback: "warn",
  project: "ok",
  reference: "",
};

/** Compact age like "3d" / "5mo" from epoch seconds. */
export function age(epochSecs: number, now = Date.now()): string {
  if (!epochSecs) return "";
  const secs = Math.max(0, Math.round(now / 1000 - epochSecs));
  if (secs < 60) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  if (days < 31) return `${days}d`;
  const months = Math.round(days / 30.4);
  if (months < 12) return `${months}mo`;
  return `${Math.round(days / 365)}y`;
}

/** `modified` frontmatter if Claude wrote one, else the file's mtime. */
export function entryAge(entry: MemoryEntry, now = Date.now()): string {
  const parsed = entry.modified ? Date.parse(entry.modified) : NaN;
  return age(Number.isNaN(parsed) ? entry.mtime : Math.round(parsed / 1000), now);
}

export function storeTitle(store: MemoryStore): string {
  if (store.kind === "agent") return `${store.label} (subagent)`;
  if (store.kind === "user") return store.label;
  return store.label;
}

/** One line about how this store is loaded, for the store view. */
export function storeSummary(store: MemoryStore): string {
  const notes = `${store.entries.length} note${store.entries.length === 1 ? "" : "s"}`;
  if (!store.exists) {
    return store.agent?.declared
      ? `No memory written yet — ${store.label} has memory enabled but hasn't saved anything.`
      : "This folder doesn't exist yet.";
  }
  if (!store.enabled) {
    return `${notes}, but auto memory is off (${store.enabled_source}), so none of it loads.`;
  }
  return store.index.exists
    ? `${notes}. MEMORY.md loads at session start; notes load when Claude follows the index.`
    : `${notes}, but there's no MEMORY.md index.`;
}

export interface EntryFilter {
  query: string;
  /** Empty = every type. */
  types: string[];
  /** Only notes missing from the index. */
  unindexed: boolean;
}

export function matchesFilter(entry: MemoryEntry, filter: EntryFilter): boolean {
  const q = filter.query.trim().toLowerCase();
  if (q && !`${entry.name} ${entry.description ?? ""} ${entry.rel}`.toLowerCase().includes(q)) {
    return false;
  }
  if (filter.types.length && !filter.types.includes(entry.kind ?? "")) return false;
  if (filter.unindexed && entry.in_index) return false;
  return true;
}

export function filterStores(stores: MemoryStore[], filter: EntryFilter): MemoryStore[] {
  const q = filter.query.trim().toLowerCase();
  const plain = !q && !filter.types.length && !filter.unindexed;
  if (plain) return stores;
  return stores
    .map((s) => ({ ...s, entries: s.entries.filter((e) => matchesFilter(e, filter)) }))
    .filter((s) => s.entries.length > 0 || (!!q && s.label.toLowerCase().includes(q)));
}

/** Counts per type across stores, for the filter chips. */
export function typeCounts(stores: MemoryStore[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const s of stores) {
    for (const e of s.entries) {
      const k = e.kind ?? "untyped";
      out[k] = (out[k] ?? 0) + 1;
    }
  }
  return out;
}

export function findEntry(
  stores: MemoryStore[],
  path: string,
): { store: MemoryStore; entry: MemoryEntry } | null {
  for (const store of stores) {
    const entry = store.entries.find((e) => e.path === path);
    if (entry) return { store, entry };
  }
  return null;
}

/** The store whose index file is `path`, if any. */
export function findIndex(stores: MemoryStore[], path: string): MemoryStore | null {
  return stores.find((s) => s.index.path === path) ?? null;
}
