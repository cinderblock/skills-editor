import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { aiListJobs, deleteSkill, discoverSkills, setSkillEnabled } from "./api";
import AiTaskDialog, { type AiTarget } from "./components/AiTaskDialog";
import EditorPane, { type EditorPaneHandle } from "./components/EditorPane";
import InstallDialog from "./components/InstallDialog";
import JobsPanel from "./components/JobsPanel";
import NewSkillDialog from "./components/NewSkillDialog";
import SettingsDialog from "./components/SettingsDialog";
import Sidebar from "./components/Sidebar";
import SyncPanel from "./components/SyncPanel";
import type { JobInfo, OpenFile, Skill, SkillGroup } from "./types";

interface Confirm {
  message: string;
  actions: { label: string; kind?: "accent" | "danger"; run: () => void }[];
}

export default function App() {
  const [groups, setGroups] = useState<SkillGroup[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [file, setFile] = useState<OpenFile | null>(null);
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
  const statusTimer = useRef<number | undefined>(undefined);
  const runningIds = useRef<Set<number>>(new Set());

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
        if (!current) return current;
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

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Poll AI jobs; when one finishes, reload the editor and rescan skills.
  useEffect(() => {
    const tick = async () => {
      try {
        const list = await aiListJobs();
        setJobs(list);
        const nowRunning = new Set(list.filter((j) => j.status === "running").map((j) => j.id));
        const finished = [...runningIds.current].filter((id) => !nowRunning.has(id));
        if (finished.length > 0) {
          setReloadToken((t) => t + 1);
          void refresh();
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
  }, [refresh, say]);

  const openFile = useCallback(
    (next: OpenFile) => {
      if (dirty && editorRef.current?.isDirty()) {
        setConfirm({
          message: "You have unsaved changes. Save them before switching files?",
          actions: [
            {
              label: "Save & open",
              kind: "accent",
              run: () => {
                void editorRef.current
                  ?.save()
                  .then(() => setFile(next))
                  .catch(() => {});
              },
            },
            { label: "Discard changes", kind: "danger", run: () => setFile(next) },
            { label: "Cancel", run: () => {} },
          ],
        });
        return;
      }
      setFile(next);
    },
    [dirty],
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
          setFile({ path: skill.skill_md, skill, group: g });
          break;
        }
      }
    },
    [refresh],
  );

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
            onClick={() => setAiTarget({ mode: "skills", file: file ?? undefined })}
          >
            AI task
          </button>
          <button className="btn" onClick={() => setShowSync(true)}>
            Sync
          </button>
          <button className="btn" onClick={() => setShowSettings(true)}>
            Settings
          </button>
          <button className="btn" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>
        <span className="statusline">{status}</span>
      </div>

      <div className="main">
        {loadError ? (
          <div className="sidebar">
            <div className="modal-error">{loadError}</div>
          </div>
        ) : (
          <Sidebar groups={groups} selectedPath={file?.path ?? null} onOpen={openFile} />
        )}
        <EditorPane
          ref={editorRef}
          file={file}
          reloadToken={reloadToken}
          onStatus={say}
          onDirtyChange={setDirty}
          onDeleteSkill={requestDelete}
          onToggleDisabled={toggleDisabled}
          onAiSelection={(f, selection) =>
            setAiTarget({ mode: "selection", file: f, selection })
          }
        />
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
          onClose={() => setShowSettings(false)}
          onSaved={() => {
            say("Settings saved");
            void refresh();
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
