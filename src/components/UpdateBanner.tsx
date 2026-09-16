import { useState } from "react";
import type { Updater } from "../updates";

function formatMb(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function UpdateBanner({
  updater,
  unsafeToRestart,
}: {
  updater: Updater;
  /** Why restarting now would lose something, or null when it's safe. */
  unsafeToRestart: string | null;
}) {
  const [showNotes, setShowNotes] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const { phase } = updater;

  if (updater.dismissed) return null;
  if (
    phase.kind !== "available" &&
    phase.kind !== "downloading" &&
    phase.kind !== "installing"
  ) {
    return null;
  }

  const { update } = phase;
  const notes = update.body?.trim();

  const start = () => {
    if (unsafeToRestart && !confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    void updater.install();
  };

  return (
    <div className="update-banner">
      <div className="update-row">
        <span className="update-text">
          Skills Editor <strong>v{update.version}</strong> is available
          <span className="update-current">you have v{update.currentVersion}</span>
        </span>
        {notes && (
          <button className="btn small" onClick={() => setShowNotes(!showNotes)}>
            {showNotes ? "Hide notes" : "Release notes"}
          </button>
        )}
        {phase.kind === "available" && (
          <>
            <button className="btn small accent" onClick={start}>
              {confirming ? "Restart anyway" : "Install & restart"}
            </button>
            <button
              className="btn small"
              onClick={() => (confirming ? setConfirming(false) : updater.dismiss())}
            >
              {confirming ? "Cancel" : "Later"}
            </button>
          </>
        )}
        {phase.kind === "downloading" && (
          <span className="update-progress">
            Downloading {formatMb(phase.downloaded)}
            {phase.total ? ` of ${formatMb(phase.total)}` : ""}…
          </span>
        )}
        {phase.kind === "installing" && (
          <span className="update-progress">Installing — the app will restart…</span>
        )}
      </div>
      {confirming && unsafeToRestart && (
        <div className="update-warning">
          Installing closes the app: {unsafeToRestart}.
        </div>
      )}
      {showNotes && notes && <pre className="update-notes">{notes}</pre>}
    </div>
  );
}
