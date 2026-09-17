import { useMemo, useState } from "react";
import { formatBytes } from "../instructionsModel";
import { TYPE_TONE, entryAge, filterStores, typeCounts, type EntryFilter } from "../memoryModel";
import type { MemoryOverview, MemorySelection, MemoryStore } from "../types";

const KIND_BADGE: Record<MemoryStore["kind"], string> = {
  project: "project",
  agent: "subagent",
  user: "user",
  other: "no project",
};

const COLLAPSED_BY_DEFAULT = new Set<MemoryStore["kind"]>(["other"]);

export default function MemorySidebar({
  overview,
  error,
  loading,
  selection,
  onSelect,
  onNew,
}: {
  overview: MemoryOverview | null;
  error: string | null;
  loading: boolean;
  selection: MemorySelection | null;
  onSelect: (sel: MemorySelection) => void;
  onNew: (store: MemoryStore) => void;
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [filter, setFilter] = useState<EntryFilter>({ query: "", types: [], unindexed: false });
  const stores = useMemo(() => filterStores(overview?.stores ?? [], filter), [overview, filter]);
  const counts = useMemo(() => typeCounts(overview?.stores ?? []), [overview]);
  const filtering = !!filter.query.trim() || filter.types.length > 0 || filter.unindexed;
  const unindexed = (overview?.stores ?? []).reduce(
    (n, s) => n + s.entries.filter((e) => !e.in_index).length,
    0,
  );

  const toggleType = (t: string) =>
    setFilter((f) => ({
      ...f,
      types: f.types.includes(t) ? f.types.filter((x) => x !== t) : [...f.types, t],
    }));

  return (
    <div className="sidebar">
      <div className="sidebar-actions">
        <input
          className="sidebar-filter"
          placeholder="Filter notes"
          value={filter.query}
          onChange={(e) => setFilter({ ...filter, query: e.target.value })}
        />
      </div>
      <div className="chip-row">
        {(overview?.types ?? []).map((t) => (
          <button
            key={t}
            className={`chip ${TYPE_TONE[t] ?? ""}${filter.types.includes(t) ? " on" : ""}`}
            onClick={() => toggleType(t)}
          >
            {t}
            <span className="chip-count">{counts[t] ?? 0}</span>
          </button>
        ))}
        {unindexed > 0 && (
          <button
            className={`chip warn${filter.unindexed ? " on" : ""}`}
            onClick={() => setFilter({ ...filter, unindexed: !filter.unindexed })}
          >
            not in index
            <span className="chip-count">{unindexed}</span>
          </button>
        )}
      </div>
      {error && <div className="modal-error sidebar-error">{error}</div>}
      {!overview && !error && <div className="group-empty">Reading memory folders…</div>}
      {overview && loading && <div className="group-note">Rescanning…</div>}
      {overview && stores.length === 0 && <div className="group-empty">Nothing matches.</div>}
      {stores.map((store) => {
        const isCollapsed = !filtering && (collapsed[store.key] ?? COLLAPSED_BY_DEFAULT.has(store.kind));
        return (
          <div className="group" key={store.key}>
            <button
              className="group-header"
              onClick={() => setCollapsed({ ...collapsed, [store.key]: !isCollapsed })}
            >
              <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
              <span className="group-label">{store.label}</span>
              <span className={`badge kind-${store.kind}`}>{KIND_BADGE[store.kind]}</span>
              {!store.enabled && <span className="badge disabled">off</span>}
              {store.warnings.length > 0 && <span className="badge warn">⚠</span>}
              <span className="group-count">{store.entries.length}</span>
            </button>
            {!isCollapsed && (
              <>
                <button
                  className={`instr-row startup${selection?.kind === "store" && selection.store === store.key ? " selected" : ""}`}
                  onClick={() => onSelect({ kind: "store", store: store.key })}
                >
                  <span className="instr-name">Index &amp; folder</span>
                  <span className="instr-meta">
                    {store.index.exists ? `${store.index.lines} lines` : "no MEMORY.md"}
                  </span>
                </button>
                {store.entries.map((e) => (
                  <button
                    key={e.path}
                    className={`instr-row${selection?.kind === "note" && selection.path === e.path ? " selected" : ""}`}
                    onClick={() => onSelect({ kind: "note", path: e.path })}
                  >
                    <span className="instr-name">{e.name}</span>
                    {e.kind && <span className={`badge ${TYPE_TONE[e.kind] ?? ""}`}>{e.kind}</span>}
                    {!e.in_index && <span className="badge warn">not in index</span>}
                    <span className="instr-meta">{entryAge(e) || formatBytes(e.bytes)}</span>
                  </button>
                ))}
                {store.entries.length === 0 && <div className="group-empty">no notes yet</div>}
                <div className="store-actions">
                  <button className="btn small" onClick={() => onNew(store)}>
                    New note
                  </button>
                </div>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
