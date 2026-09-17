import { useState } from "react";
import { instructionsSetAutoMemory, memoryIndexEntry } from "../api";
import { formatBytes, roughTokens } from "../instructionsModel";
import { TYPE_TONE, entryAge, storeSummary } from "../memoryModel";
import type { MemoryStore } from "../types";

/** A memory folder: its index, what's in it, and whether they agree. */
export default function MemoryStoreView({
  store,
  onOpenNote,
  onOpenIndex,
  onNew,
  onStatus,
  onChanged,
}: {
  store: MemoryStore;
  onOpenNote: (path: string) => void;
  onOpenIndex: (path: string) => void;
  onNew: (store: MemoryStore) => void;
  onStatus: (msg: string) => void;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unindexed = store.entries.filter((e) => !e.in_index);

  const run = async (action: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    try {
      onStatus(await action());
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="editor-pane">
      <div className="editor-header">
        <div className="editor-title">
          <span className="editor-skill">{store.label}</span>
          <span className="editor-file">
            {store.kind === "agent" ? `subagent memory (${store.agent?.scope})` : "auto memory"}
          </span>
          {!store.enabled && <span className="badge disabled">off</span>}
        </div>
        <div className="editor-actions">
          <button className="btn accent" onClick={() => onNew(store)}>
            New note
          </button>
        </div>
      </div>
      <div className="editor-path">{store.dir}</div>

      <div className="hook-body">
        <div className="hook-hint">{storeSummary(store)}</div>
        {store.agent && (
          <div className="hook-hint">
            {store.agent.declared ? (
              <>
                Declared by <code>{store.agent.file}</code> with <code>memory: {store.agent.scope}</code>.
              </>
            ) : (
              <>
                No subagent currently declares <code>memory: {store.agent.scope}</code> with this name — these
                notes are left over.
              </>
            )}
          </div>
        )}
        {store.warnings.map((w) => (
          <div key={w} className="banner warn">
            {w}
          </div>
        ))}
        {error && <div className="banner error">{error}</div>}

        <div className="hook-section">
          <div className="section-title">Index</div>
          {store.index.exists ? (
            <div className="memory-state">
              <button className="link-btn" onClick={() => onOpenIndex(store.index.path)}>
                MEMORY.md
              </button>
              <span className="dim">
                {store.index.lines} lines · {formatBytes(store.index.loaded_bytes)} loads (
                {roughTokens(store.index.loaded_bytes)})
                {store.index.truncated && " — over the limit, the rest is dropped"}
              </span>
            </div>
          ) : (
            <div className="hook-hint">
              No MEMORY.md yet. Claude loads only the index at startup, so notes without an index entry are
              easy to miss.
            </div>
          )}
          {unindexed.length > 0 && (
            <div className="memory-state">
              <span className="warn">
                {unindexed.length} note{unindexed.length === 1 ? "" : "s"} not in the index
              </span>
              <button
                className="btn small"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    for (const e of unindexed) await memoryIndexEntry(e.path);
                    return `Added ${unindexed.length} note(s) to MEMORY.md`;
                  })
                }
              >
                Add them all
              </button>
            </div>
          )}
        </div>

        <div className="hook-section">
          <div className="section-title">Notes</div>
          <table className="startup-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Type</th>
                <th>What it says</th>
                <th className="num">Age</th>
              </tr>
            </thead>
            <tbody>
              {store.entries.map((e) => (
                <tr key={e.path}>
                  <td>
                    <button className="link-btn" onClick={() => onOpenNote(e.path)}>
                      {e.name}
                    </button>
                    {!e.in_index && <span className="badge warn">not in index</span>}
                  </td>
                  <td>{e.kind && <span className={`badge ${TYPE_TONE[e.kind] ?? ""}`}>{e.kind}</span>}</td>
                  <td className="dim">{e.description ?? ""}</td>
                  <td className="num">{entryAge(e)}</td>
                </tr>
              ))}
              {store.entries.length === 0 && (
                <tr>
                  <td colSpan={4} className="dim">
                    Nothing saved here yet.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="hook-section">
          <div className="section-title">Auto memory</div>
          <div className="memory-state">
            <span>
              {store.enabled ? "On" : "Off"} <span className="dim">— from {store.enabled_source}</span>
            </span>
            <button
              className="btn small"
              disabled={busy}
              onClick={() =>
                void run(() => instructionsSetAutoMemory(store.project_dir, !store.enabled))
              }
            >
              {store.enabled ? "Turn off" : "Turn on"}
              {store.project_dir ? " for this project" : " everywhere"}
            </button>
          </div>
          <div className="hook-hint">
            Turning auto memory off also stops subagents from using their memory folders.
          </div>
        </div>
      </div>
    </div>
  );
}
