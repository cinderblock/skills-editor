import { useState } from "react";
import { aiCancelJob, aiClearFinished, aiJobOutput } from "../api";
import type { JobInfo } from "../types";

const STATUS_LABEL: Record<string, string> = {
  running: "running",
  done: "done",
  failed: "failed",
  cancelled: "cancelled",
};

export default function JobsPanel({
  jobs,
  onChanged,
}: {
  jobs: JobInfo[];
  onChanged: () => void;
}) {
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [output, setOutput] = useState("");

  if (jobs.length === 0) return null;

  const toggle = async (job: JobInfo) => {
    if (expandedId === job.id) {
      setExpandedId(null);
      return;
    }
    setExpandedId(job.id);
    try {
      setOutput(await aiJobOutput(job.id));
    } catch {
      setOutput(job.output_tail);
    }
  };

  const running = jobs.filter((j) => j.status === "running").length;

  return (
    <div className="jobs-panel">
      <div className="jobs-header">
        <span className="jobs-title">
          AI jobs {running > 0 ? `— ${running} running` : ""}
        </span>
        <button
          className="btn small"
          onClick={() => {
            void aiClearFinished().then(onChanged);
            setExpandedId(null);
          }}
        >
          Clear finished
        </button>
      </div>
      <div className="jobs-list">
        {jobs.map((job) => (
          <div key={job.id} className={`job status-${job.status}`}>
            <button className="job-row" onClick={() => void toggle(job)}>
              <span className={`job-status ${job.status}`}>
                {job.status === "running" ? (
                  <span className="spinner" />
                ) : (
                  STATUS_LABEL[job.status]
                )}
              </span>
              <span className="job-label">{job.label}</span>
              <span className="job-cwd">{job.cwd}</span>
            </button>
            {job.status === "running" && (
              <button
                className="btn small danger"
                onClick={() => void aiCancelJob(job.id).then(onChanged)}
              >
                Cancel
              </button>
            )}
            {expandedId === job.id && (
              <div className="job-detail">
                <div className="job-prompt">
                  <strong>Prompt</strong>
                  <pre>{job.prompt}</pre>
                </div>
                <div className="job-output">
                  <strong>Output</strong>
                  <pre>{output || job.output_tail || "(no output yet)"}</pre>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
