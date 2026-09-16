import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { aiListJobs, deleteSkill, discoverSkills, hooksOverview, setSkillEnabled } from "./api";
import AiTaskDialog, { type AiTarget } from "./components/AiTaskDialog";
import EditorPane, { type EditorPaneHandle } from "./components/EditorPane";
import HookEditor, { type HookEditorHandle } from "./components/HookEditor";
import HooksSidebar from "./components/HooksSidebar";
import InstallDialog from "./components/InstallDialog";
import JobsPanel from "./components/JobsPanel";
import NewSkillDialog from "./components/NewSkillDialog";
import SettingsDialog from "./components/SettingsDialog";
import Sidebar from "./components/Sidebar";
import SyncPanel from "./components/SyncPanel";
import UpdateBanner from "./components/UpdateBanner";
import type {
  HookGroup,
  HookSelection,
  HooksOverview,
  JobInfo,
  OpenFile,
  Skill,
  SkillGroup,
} from "./types";
import { useUpdater } from "./updates";

interface Confirm {
  message: string;
  actions: { label: string; kind?: "accent" | "danger"; run: () => void }[];
}

type View = "skills" | "hooks";

/** A stand-in skill so hook scripts can use the file editor (and AI edits). */
function scriptFile(path: string, editable: boolean, group: HookGroup | null): OpenFile {
  const sep = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  const dir = path.slice(0, sep);
  const name = path.slice(sep + 1);
  const skill: Skill = {
    id: path,
    name,
    description: "",
    dir,
    skill_md: "",
    files: [name],
    single_file: true,
    editable,
    disabled: false,
  };
  return {
    path,
    skill,
    kind: "script",
    group: {
      key: `hooks:${group?.key ?? dir}`,
      kind: "extra",
      label: group?.label ?? "hooks",
      detail: group?.detail ?? dir,
      skills: [],
    },
  };
}

export default function App() {
  const [view, setView] = useState<View>("skills");
  const [groups, setGroups] = useState<SkillGroup[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [hooks, setHooks] = useState<HooksOverview | null>(null);
  const [hooksError, setHooksError] = useState<string | null>(null);
  const [file, setFile] = useState<OpenFile | null>(null);
  const [hookSel, setHookSel] = useState<HookSelection | null>(null);
  /** Which editor owns the main pane. */
  const [main, setMain] = useState<"file" | "hook">("file");
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState("");
  const [jobs, setJobs] = useState<JobInfo[]>([]);
  const [reloadToken, setReloadToken] = useState(0);
  const [showInstall, setShowInstall] = useState(false);
  const [showSync, setShowSync] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showNewSkill, setShowNewSkill] = useState(false);
  const [aiTarget, setAiTarget] = useState<AiTarget | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const editorRef = useRef<EditorPaneHandle>(null);
  const hookEditorRef = useRef<HookEditorHandle>(null);
  const statusTimer = useRef<number | undefined>(undefined);
  const runningIds = useRef<Set<number>>(new Set());
  const updater = useUpdater();

  const say = useCallback((msg: string) => {
    setStatus(msg);
    window.clearTimeout(statusTimer.current);
    statusTimer.current = window.setTimeout(() => setStatus(""), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const found = await discoverSkills();
      setGroups(found);
      setLoadError(null);
      // Keep the open file's skill object fresh (files list may have changed).
      setFile((current) => {
        if (!current || current.kind === "script") return current;
        for (const g of found) {
          const skill = g.skills.find((s) => s.id === current.skill.id);
          if (skill) return { path: current.path, skill, group: g };
        }
        return current;
      });
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);

  const refreshHooks = useCallback(async (): Promise<HooksOverview | null> => {
    try {
      const ov = await hooksOverview();
      setHooks(ov);
      setHooksError(null);
      return ov;
    } catch (e) {
      setHooksError(String(e));
      return null;
    }
  }, []);

  const refreshAll = useCallback(() => {
    void refresh();
    void refreshHooks();
  }, [refresh, refreshHooks]);

  useEffect(() => {
    refreshAll();
    // Claude Code and other tools edit these files too — rescan on focus.
    window.addEventListener("focus", refreshAll);
    return () => window.removeEventListener("focus", refreshAll);
  }, [refreshAll]);

  // Poll AI jobs; when one finishes, reload the editor and rescan.
  useEffect(() => {
    const tick = async () => {
      try {
        const list = await aiListJobs();
        setJobs(list);
        const nowRunning = new Set(list.filter((j) => j.status === "running").map((j) => j.id));
        const finished = [...runningIds.current].filter((id) => !nowRunning.has(id));
        if (finished.length > 0) {
          setReloadToken((t) => t + 1);
          refreshAll();
          const failed = list.filter(
            (j) => finished.includes(j.id) && j.status === "failed",
          );
          say(
            failed.length > 0
              ? `AI job${failed.length > 1 ? "s" : ""} failed — see the jobs panel`
              : "AI job finished — files updated",
          );
        }
        runningIds.current = nowRunning;
      } catch {
        // Backend not ready yet; ignore.
      }
    };
    void tick();
    const interval = window.setInterval(() => void tick(), 2000);
    return () => window.clearInterval(interval);
  }, [refreshAll, say]);

  /** Run `next` once unsaved work in whichever editor is active is dealt with. */
  const guard = useCallback(
    (next: () => void) => {
      const active = main === "hook" ? hookEditorRef.current : editorRef.current;
      const proceed = () => {
        setDirty(false);
        next();
      };
      if (!dirty || !active?.isDirty()) {
        next();
        return;
      }
      setConfirm({
        message: "You have unsaved changes. Save them first?",
        actions: [
          {
            label: "Save & continue",
            kind: "accent",
            run: () => {
              void active
                .save()
                .then(() => {
                  if (!active.isDirty()) proceed();
                })
                .catch(() => {});
            },
          },
          { label: "Discard changes", kind: "danger", run: proceed },
          { label: "Cancel", run: () => {} },
        ],
      });
    },
    [dirty, main],
  );

  const openFile = useCallback(
    (next: OpenFile) =>
      guard(() => {
        setFile(next);
        setMain("file");
      }),
    [guard],
  );

  const selectHook = useCallback(
    (sel: HookSelection) =>
      guard(() => {
        setHookSel(sel);
        setMain("hook");
      }),
    [guard],
  );

  const openScript = useCallback(
    (path: string, editable: boolean, group: HookGroup | null) =>
      openFile(scriptFile(path, editable, group)),
    [openFile],
  );

  const requestDelete = useCallback(
    (skill: Skill) => {
      setConfirm({
        message: `Delete the skill "${skill.name}" and all its files?\n${skill.dir}`,
        actions: [
          {
            label: "Delete",
            kind: "danger",
            run: () => {
              void deleteSkill(skill.dir)
                .then(() => {
                  setFile((f) => (f?.skill.id === skill.id ? null : f));
                  say(`Deleted ${skill.name}`);
                  return refresh();
                })
                .catch((e) => say(String(e)));
            },
          },
          { label: "Cancel", run: () => {} },
        ],
      });
    },
    [refresh, say],
  );

  const toggleDisabled = useCallback(
    (skill: Skill) => {
      void setSkillEnabled(skill.dir, skill.disabled)
        .then((msg) => {
          say(msg);
          return refresh();
        })
        .catch((e) => say(String(e)));
    },
    [refresh, say],
  );

  const openCreated = useCallback(
    async (skillMdPath: string) => {
      await refresh();
      const found = await discoverSkills();
      for (const g of found) {
        const skill = g.skills.find((s) => s.skill_md === skillMdPath);
        if (skill) {
          openFile({ path: skill.skill_md, skill, group: g });
          break;
        }
      }
    },
    [refresh, openFile],
  );

  const running = jobs.filter((j) => j.status === "running").length;
  const unsafeToRestart =
    [
      dirty ? "you have unsaved changes" : null,
      running > 0 ? `${running} AI job${running > 1 ? "s are" : " is"} still running` : null,
    ]
      .filter(Boolean)
      .join(" and ") || null;

  return (
    <div className="app">
      <div className="topbar">
        <span className="app-title">Skills Editor</span>
        <div className="topbar-actions">
          <button className="btn" onClick={() => setShowNewSkill(true)}>
            New skill
          </button>
          <button className="btn" onClick={() => setShowInstall(true)}>
            Install
          </button>
          <button
            className="btn"
            onClick={() =>
              setAiTarget({ mode: "skills", file: file && file.kind !== "script" ? file : undefined })
            }
          >
            AI task
          </button>
          <button className="btn" onClick={() => setShowSync(true)}>
            Sync
          </button>
          <button className="btn" onClick={() => setShowSettings(true)}>
            Settings
          </button>
          <button className="btn" onClick={refreshAll}>
            Refresh
          </button>
        </div>
        <span className="statusline">{status}</span>
      </div>

      <UpdateBanner updater={updater} unsafeToRestart={unsafeToRestart} />

      <div className="main">
        <div className="sidebar-shell">
          <div className="sidebar-tabs">
            {(["skills", "hooks"] as View[]).map((v) => (
              <button
                key={v}
                className={`sidebar-tab${view === v ? " active" : ""}`}
                onClick={() => setView(v)}
              >
                {v === "skills" ? "Skills" : "Hooks"}
              </button>
            ))}
          </div>
          {view === "skills" ? (
            loadError ? (
              <div className="sidebar">
                <div className="modal-error sidebar-error">{loadError}</div>
              </div>
            ) : (
              <Sidebar
                groups={groups}
                selectedPath={main === "file" ? (file?.path ?? null) : null}
                onOpen={openFile}
              />
            )
          ) : (
            <HooksSidebar
              overview={hooks}
              error={hooksError}
              selection={main === "hook" ? hookSel : null}
              selectedPath={main === "file" ? (file?.path ?? null) : null}
              onSelect={selectHook}
              onOpenScript={(group, s) => openScript(s.path, s.editable, group)}
              onNew={() => {
                const current =
                  hookSel?.kind === "handler" ? hookSel.file : null;
                selectHook({ kind: "new", file: current });
              }}
            />
          )}
        </div>
        {main === "hook" && hookSel ? (
          <HookEditor
            key={JSON.stringify(hookSel)}
            ref={hookEditorRef}
            overview={hooks}
            selection={hookSel}
            reload={refreshHooks}
            onSelect={(sel) => {
              setHookSel(sel);
              if (!sel) setMain("file");
            }}
            onStatus={say}
            onDirtyChange={setDirty}
            onOpenScript={openScript}
          />
        ) : (
          <EditorPane
            ref={editorRef}
            file={main === "file" ? file : null}
            reloadToken={reloadToken}
            onStatus={say}
            onDirtyChange={setDirty}
            onDeleteSkill={requestDelete}
            onToggleDisabled={toggleDisabled}
            onAiSelection={(f, selection) =>
              setAiTarget({ mode: "selection", file: f, selection })
            }
          />
        )}
      </div>

      <JobsPanel jobs={jobs} onChanged={() => void aiListJobs().then(setJobs)} />

      {showInstall && (
        <InstallDialog
          groups={groups}
          onClose={() => setShowInstall(false)}
          onInstalled={(msg) => {
            say(msg);
            void refresh();
          }}
        />
      )}
      {showSync && <SyncPanel onClose={() => setShowSync(false)} onStatus={say} />}
      {showSettings && (
        <SettingsDialog
          updater={updater}
          onClose={() => setShowSettings(false)}
          onSaved={() => {
            say("Settings saved");
            refreshAll();
          }}
        />
      )}
      {showNewSkill && (
        <NewSkillDialog
          groups={groups}
          onClose={() => setShowNewSkill(false)}
          onCreated={(path) => void openCreated(path)}
        />
      )}
      {aiTarget && (
        <AiTaskDialog
          target={aiTarget}
          groups={groups}
          onClose={() => setAiTarget(null)}
          onStarted={(count) =>
            say(`Started ${count} AI job${count > 1 ? "s" : ""}`)
          }
        />
      )}
      {confirm && (
        <div className="modal-backdrop" onClick={() => setConfirm(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <p className="confirm-message">{confirm.message}</p>
            <div className="modal-actions">
              {confirm.actions.map((a) => (
                <button
                  key={a.label}
                  className={`btn ${a.kind ?? ""}`}
                  onClick={() => {
                    setConfirm(null);
                    a.run();
                  }}
                >
                  {a.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
