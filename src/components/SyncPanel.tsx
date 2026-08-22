import { useCallback, useEffect, useState } from "react";
import {
  syncFetch,
  syncInit,
  syncPull,
  syncPush,
  syncSnapshot,
  syncStatus,
} from "../api";
import type { SyncStatus } from "../types";

export default function SyncPanel({
  onClose,
  onStatus,
}: {
  onClose: () => void;
  onStatus: (msg: string) => void;
}) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await syncStatus());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const act = async (name: string, fn: () => Promise<string>) => {
    setBusy(name);
    setError(null);
    try {
      const msg = await fn();
      onStatus(msg);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Skills tracking repo</h2>
        {status && (
          <div className="sync-status">
            <div className="sync-line">
              <span className="sync-key">Repo</span>
              <span>{status.repo_path}</span>
            </div>
            <div className="sync-line">
              <span className="sync-key">Host</span>
              <span>{status.hostname}</span>
            </div>
            {status.initialized ? (
              <>
                <div className="sync-line">
                  <span className="sync-key">Branch</span>
                  <span>
                    {status.branch ?? "(detached)"}
                    {!status.on_host_branch && (
                      <span className="badge warn">not this host's branch</span>
                    )}
                  </span>
                </div>
                {status.last_commit && (
                  <div className="sync-line">
                    <span className="sync-key">Last</span>
                    <span>{status.last_commit}</span>
                  </div>
                )}
                <div className="sync-line">
                  <span className="sync-key">Remote</span>
                  <span>{status.remote ?? "none — set one in Settings"}</span>
                </div>
                {status.branches.length > 0 && (
                  <div className="sync-line">
                    <span className="sync-key">Branches</span>
                    <span>{status.branches.join(", ")}</span>
                  </div>
                )}
              </>
            ) : (
              <div className="sync-line">
                <span className="sync-key">State</span>
                <span>not initialized yet</span>
              </div>
            )}
          </div>
        )}
        {error && <div className="modal-error">{error}</div>}
        <div className="sync-actions">
          {status && !status.initialized && (
            <button
              className="btn accent"
              disabled={busy !== null}
              onClick={() => void act("init", syncInit)}
            >
              {busy === "init" ? "Initializing…" : "Initialize repo"}
            </button>
          )}
          {status?.initialized && (
            <>
              <button
                className="btn accent"
                disabled={busy !== null}
                onClick={() => void act("snapshot", syncSnapshot)}
              >
                {busy === "snapshot" ? "Snapshotting…" : "Snapshot now"}
              </button>
              <button
                className="btn"
                disabled={busy !== null}
                onClick={() => void act("fetch", syncFetch)}
              >
                Fetch
              </button>
              <button
                className="btn"
                disabled={busy !== null}
                onClick={() => void act("pull", syncPull)}
              >
                Pull (ff-only)
              </button>
              <button
                className="btn"
                disabled={busy !== null}
                onClick={() => void act("push", syncPush)}
              >
                Push
              </button>
            </>
          )}
        </div>
        <p className="modal-hint">
          Each host snapshots to its own branch ({status?.hostname}). Share via a
          common remote, then cherry-pick skills between host branches with
          normal git tooling.
        </p>
        <div className="modal-actions">
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
