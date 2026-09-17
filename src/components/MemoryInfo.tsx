import { useState } from "react";
import { memoryDelete, memoryIndexEntry } from "../api";
import { formatBytes } from "../instructionsModel";
import { entryAge } from "../memoryModel";
import type { MemoryEntry, MemoryStore } from "../types";

/** Details strip for one memory note. */
export default function MemoryInfo({
  store,
  entry,
  onStatus,
  onChanged,
  onClosed,
}: {
  store: MemoryStore;
  entry: MemoryEntry;
  onStatus: (msg: string) => void;
  onChanged: () => void;
  onClosed: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const [alsoIndex, setAlsoIndex] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<string>, closing = false) => {
    setBusy(true);
    setError(null);
    try {
      onStatus(await action());
      if (closing) onClosed();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="instr-info">
      <div className="instr-info-row">
        <span>
          Claude wrote this for itself in <strong>{store.label}</strong>. It reads it when the index points
          here.
        </span>
        <span className="instr-meta">
          {entryAge(entry) && `${entryAge(entry)} old · `}
          {entry.lines} lines · {formatBytes(entry.bytes)}
        </span>
      </div>
      {!entry.has_frontmatter && (
        <div className="instr-info-row warn">
          ⚠ No frontmatter — add name, description and type so Claude can tell what this is.
        </div>
      )}
      {!store.enabled && (
        <div className="instr-info-row warn">⚠ Auto memory is off ({store.enabled_source}) — this never loads.</div>
      )}
      {!entry.in_index && (
        <div className="instr-info-row">
          <span className="warn">⚠ Not mentioned in MEMORY.md, so Claude is unlikely to find it.</span>
          <button className="btn small" disabled={busy} onClick={() => void run(() => memoryIndexEntry(entry.path))}>
            Add to index
          </button>
        </div>
      )}
      {error && <div className="instr-info-row warn">{error}</div>}
      <div className="instr-info-row actions">
        {confirm ? (
          <>
            <label className="check-label">
              <input type="checkbox" checked={alsoIndex} onChange={(e) => setAlsoIndex(e.target.checked)} />
              also remove its line from MEMORY.md
            </label>
            <button
              className="btn small danger"
              disabled={busy}
              onClick={() => void run(() => memoryDelete(entry.path, alsoIndex), true)}
            >
              Delete {entry.rel}
            </button>
            <button className="btn small" onClick={() => setConfirm(false)}>
              Keep
            </button>
          </>
        ) : (
          <button className="btn small danger" onClick={() => setConfirm(true)}>
            Forget this
          </button>
        )}
      </div>
    </div>
  );
}
