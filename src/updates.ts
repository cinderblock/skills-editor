import { useCallback, useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** First automatic check after launch, then periodically. */
const FIRST_CHECK_MS = 15_000;
const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; downloaded: number; total: number | null }
  | { kind: "installing"; update: Update }
  | { kind: "error"; message: string };

export interface Updater {
  phase: UpdatePhase;
  /** True when the user hid the banner for the version on offer. */
  dismissed: boolean;
  /** Manual check; surfaces errors (automatic checks stay quiet). */
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

export function useUpdater(): Updater {
  const [phase, setPhase] = useState<UpdatePhase>({ kind: "idle" });
  const [dismissedVersion, setDismissedVersion] = useState<string | null>(null);
  const busy = useRef(false);

  const runCheck = useCallback(async (manual: boolean) => {
    if (busy.current) return;
    busy.current = true;
    if (manual) setPhase({ kind: "checking" });
    try {
      const update = await check();
      if (update) setPhase({ kind: "available", update });
      else if (manual) setPhase({ kind: "none" });
    } catch (e) {
      // Automatic checks fail quietly (offline, no release published yet).
      if (manual) setPhase({ kind: "error", message: String(e) });
    } finally {
      busy.current = false;
    }
  }, []);

  useEffect(() => {
    // Dev builds share the release version, so auto-checks there are noise.
    if (import.meta.env.DEV) return;
    const first = window.setTimeout(() => void runCheck(false), FIRST_CHECK_MS);
    const every = window.setInterval(() => void runCheck(false), CHECK_EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, [runCheck]);

  const install = useCallback(async () => {
    if (phase.kind !== "available") return;
    const { update } = phase;
    let downloaded = 0;
    let total: number | null = null;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          setPhase({ kind: "downloading", update, downloaded: 0, total });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setPhase({ kind: "downloading", update, downloaded, total });
        } else if (event.event === "Finished") {
          setPhase({ kind: "installing", update });
        }
      });
      // On Windows the installer usually exits the app itself; this covers
      // the platforms (and install modes) where it doesn't.
      await relaunch();
    } catch (e) {
      setPhase({ kind: "error", message: `Update failed: ${e}` });
    }
  }, [phase]);

  const offered =
    phase.kind === "available" || phase.kind === "downloading" || phase.kind === "installing"
      ? phase.update.version
      : null;

  return {
    phase,
    dismissed: offered !== null && offered === dismissedVersion,
    // An explicit check re-shows a banner the user had put off.
    checkNow: () => {
      setDismissedVersion(null);
      return runCheck(true);
    },
    install,
    dismiss: () => setDismissedVersion(offered),
  };
}
