import { useState } from "react";
import { instructionsSetAutoMemory } from "../api";
import { formatBytes, roughTokens, startupTotal } from "../instructionsModel";
import type { InstrGroup } from "../types";

/** What a new session in this scope reads before your first message. */
export default function StartupContext({
  group,
  onOpenPath,
  onStatus,
  onChanged,
}: {
  group: InstrGroup;
  onOpenPath: (path: string) => void;
  onStatus: (msg: string) => void;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const total = startupTotal(group.startup);
  const mem = group.auto_memory;
  const scope = group.kind === "project" ? group.project_dir : null;

  const toggleMemory = async () => {
    if (!mem) return;
    setBusy(true);
    setError(null);
    try {
      onStatus(await instructionsSetAutoMemory(scope, !mem.enabled));
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
          <span className="editor-skill">{group.label}</span>
          <span className="editor-file">startup context</span>
        </div>
      </div>
      <div className="editor-path">{group.detail}</div>
      <div className="hook-body">
        <div className="hook-hint">
          {group.kind === "user"
            ? "What every Claude Code session starts with, before any project files."
            : "What a session started in this folder reads before your first message, in load order."}{" "}
          Totals are {roughTokens(total)} ({formatBytes(total)}) — a rough estimate. Order within a folder is
          approximate; run <code>/context</code> in a session for the exact list.
        </div>
        <table className="startup-table">
          <thead>
            <tr>
              <th>File</th>
              <th className="num">Lines</th>
              <th className="num">Loaded</th>
              <th className="num">Tokens</th>
            </tr>
          </thead>
          <tbody>
            {group.startup.map((e, i) => (
              <tr key={`${e.path}|${i}`}>
                <td style={{ paddingLeft: 8 + e.depth * 16 }}>
                  {e.path ? (
                    <button className="link-btn" onClick={() => onOpenPath(e.path)}>
                      {e.label}
                    </button>
                  ) : (
                    <span>{e.label}</span>
                  )}
                  {e.note && <span className="badge warn">{e.note}</span>}
                </td>
                <td className="num">{e.lines}</td>
                <td className="num">
                  {formatBytes(e.loaded_bytes)}
                  {e.loaded_bytes < e.bytes && <span className="dim"> of {formatBytes(e.bytes)}</span>}
                </td>
                <td className="num">{roughTokens(e.loaded_bytes).replace(" tokens", "")}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {mem && (
          <div className="hook-section">
            <div className="section-title">Auto memory</div>
            <div className="memory-state">
              <span>
                {mem.enabled ? "On" : "Off"} <span className="dim">— from {mem.source}</span>
              </span>
              <button className="btn small" disabled={busy} onClick={() => void toggleMemory()}>
                {mem.enabled ? "Turn off" : "Turn on"}
                {group.kind === "project" ? " for this project" : " everywhere"}
              </button>
            </div>
            <div className="hook-hint">
              {group.kind === "project" ? (
                <>
                  Folder: <code>{mem.dir}</code>
                  {!mem.exists && " (nothing saved yet)"}. The switch writes{" "}
                  <code>autoMemoryEnabled</code> to this project's <code>.claude/settings.local.json</code>.
                </>
              ) : (
                <>
                  Saved per project under <code>{mem.dir}</code>. The switch edits{" "}
                  <code>autoMemoryEnabled</code> in <code>~/.claude/settings.json</code>; projects can override it.
                </>
              )}
            </div>
            {error && <div className="banner error">{error}</div>}
          </div>
        )}
        {group.notes.map((n) => (
          <div key={n} className="hook-hint">
            {n}
          </div>
        ))}
      </div>
    </div>
  );
}
