import { useState } from "react";
import { formatBytes, loadsText } from "../instructionsModel";
import type { InstrFile, InstrGroup } from "../types";

/** Details strip shown above an open instruction/memory file. */
export default function InstructionInfo({
  group,
  file,
  onOpenPath,
  onDelete,
}: {
  group: InstrGroup;
  file: InstrFile;
  onOpenPath: (path: string) => void;
  onDelete: (file: InstrFile) => void;
}) {
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="instr-info">
      <div className="instr-info-row">
        <span>{loadsText(file)}</span>
        <span className="instr-meta">
          {file.lines} lines · {formatBytes(file.bytes)}
        </span>
      </div>
      {file.memory?.description && (
        <div className="instr-info-row dim">{file.memory.description}</div>
      )}
      {file.applies_to.length > 0 && (
        <div className="instr-info-row dim">Applies to: {file.applies_to.join(", ")}</div>
      )}
      {file.imports.length > 0 && (
        <div className="instr-info-row wrap">
          <span className="dim">Imports:</span>
          {file.imports.map((i) =>
            i.exists ? (
              <button key={i.raw} className="link-btn" onClick={() => onOpenPath(i.path)}>
                @{i.raw}
                {i.external && <span className="badge warn">outside project</span>}
              </button>
            ) : (
              <span key={i.raw} className="badge disabled">
                @{i.raw} missing
              </span>
            ),
          )}
        </div>
      )}
      {file.warnings.map((w) => (
        <div key={w} className="instr-info-row warn">
          ⚠ {w}
        </div>
      ))}
      {file.editable && group.kind !== "managed" && (
        <div className="instr-info-row actions">
          {confirm ? (
            <>
              <span className="warn">Delete {file.label}? This can't be undone from the app.</span>
              <button className="btn small danger" onClick={() => onDelete(file)}>
                Delete
              </button>
              <button className="btn small" onClick={() => setConfirm(false)}>
                Keep
              </button>
            </>
          ) : (
            <button className="btn small danger" onClick={() => setConfirm(true)}>
              Delete file
            </button>
          )}
        </div>
      )}
    </div>
  );
}
