import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Stage = "idle" | "available" | "downloading" | "ready" | "failed";

/**
 * own-brew updating itself.
 *
 * Deliberately quiet: a package manager that nags about its own version while
 * you are trying to fix something else is an irritation. The check runs once
 * at startup and stays silent unless there is something to say.
 */
export function AppUpdate() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [stage, setStage] = useState<Stage>("idle");
  const [progress, setProgress] = useState(0);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    // A missing or unreachable endpoint is not worth reporting — the app works
    // perfectly well without ever updating itself.
    check()
      .then((found) => {
        if (found) {
          setUpdate(found);
          setStage("available");
        }
      })
      .catch(() => undefined);
  }, []);

  if (!update || dismissed || stage === "idle") return null;

  const install = async () => {
    setStage("downloading");
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) setProgress(Math.round((downloaded / total) * 100));
        }
      });
      setStage("ready");
    } catch {
      setStage("failed");
    }
  };

  return (
    <div className="appupdate">
      {stage === "available" && (
        <>
          <div className="appupdate__text">
            own-brew <span className="mono">{update.version}</span> is available
          </div>
          <div className="appupdate__actions">
            <button className="btn btn--sm btn--primary" onClick={() => void install()}>
              Update
            </button>
            <button className="btn btn--sm" onClick={() => setDismissed(true)}>
              Later
            </button>
          </div>
        </>
      )}

      {stage === "downloading" && (
        <>
          <div className="appupdate__text">Downloading… {progress > 0 && `${progress}%`}</div>
          <div className="progress" style={{ maxWidth: "100%" }}>
            <div className="progress__fill" style={{ width: `${progress}%` }} />
          </div>
        </>
      )}

      {stage === "ready" && (
        <>
          <div className="appupdate__text">Installed — restart to use it</div>
          <div className="appupdate__actions">
            <button className="btn btn--sm btn--primary" onClick={() => void relaunch()}>
              Restart
            </button>
            <button className="btn btn--sm" onClick={() => setDismissed(true)}>
              Later
            </button>
          </div>
        </>
      )}

      {stage === "failed" && (
        <div className="appupdate__text" style={{ color: "var(--rust)" }}>
          Update failed — it will be offered again next launch
        </div>
      )}
    </div>
  );
}
