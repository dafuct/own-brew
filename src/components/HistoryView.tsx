import { useEffect, useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, Change, Operation } from "../api/types";
import { ErrorBanner } from "./ErrorBanner";
import { relativeTime } from "./Tags";

const ARROW: Record<Change["change"], string> = {
  installed: "+",
  removed: "−",
  upgraded: "→",
  downgraded: "↩",
  changed: "~",
};

const TONE: Record<Change["change"], string> = {
  installed: "var(--sage)",
  removed: "var(--rust)",
  upgraded: "var(--accent)",
  downgraded: "var(--amber)",
  changed: "var(--text-muted)",
};

export function HistoryList({ refreshToken }: { refreshToken: number }) {
  const [operations, setOperations] = useState<Operation[] | null>(null);
  const [error, setError] = useState<BrewError | null>(null);
  const [expanded, setExpanded] = useState<number | null>(null);

  useEffect(() => {
    api
      .history(150)
      .then((ops) => {
        setOperations(ops);
        setError(null);
      })
      .catch((e) => setError(asBrewError(e)));
  }, [refreshToken]);

  return (
    <>
      <header className="header">
        <div style={{ fontSize: "var(--step-1)", fontWeight: 500 }}>What changed</div>
        <div className="header__spacer" />
        {operations && (
          <span style={{ fontSize: "var(--step--1)", color: "var(--text-faint)" }}>
            {operations.length} operation{operations.length === 1 ? "" : "s"} recorded
          </span>
        )}
      </header>

      {error && <ErrorBanner error={error} context="Couldn’t read history" />}

      <div className="scroller">
        {!operations ? (
          <div className="skeleton" />
        ) : operations.length === 0 ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ◷
            </div>
            <div>
              Nothing recorded yet.
              <br />
              <span style={{ fontSize: "var(--step--1)", color: "var(--text-faint)" }}>
                Every install, upgrade and removal you run here is logged with exactly what it
                changed.
              </span>
            </div>
          </div>
        ) : (
          operations.map((op) => {
            const open = expanded === op.id;
            return (
              <div key={op.id} className="op">
                <button className="op__head" onClick={() => setExpanded(open ? null : op.id)}>
                  <span
                    className="op__status"
                    style={{
                      color: op.cancelled
                        ? "var(--text-faint)"
                        : op.success
                          ? "var(--sage)"
                          : "var(--rust)",
                    }}
                    aria-hidden
                  >
                    {op.cancelled ? "◦" : op.success ? "✓" : "✕"}
                  </span>
                  <span className="op__command mono">{op.command}</span>
                  {op.changes.length > 0 && (
                    <span className="op__count mono">
                      {op.changes.length} change{op.changes.length === 1 ? "" : "s"}
                    </span>
                  )}
                  <span className="op__when">{relativeTime(op.startedAt)}</span>
                </button>

                {open && (
                  <div className="op__body">
                    {op.error && <div className="banner__detail">{op.error}</div>}
                    {op.changes.length === 0 ? (
                      <div style={{ color: "var(--text-faint)", fontSize: "var(--step--1)" }}>
                        Nothing on this machine changed.
                      </div>
                    ) : (
                      op.changes.map((change) => (
                        <div className="change" key={`${change.kind}:${change.package}`}>
                          <span
                            className="change__glyph mono"
                            style={{ color: TONE[change.change] }}
                            aria-hidden
                          >
                            {ARROW[change.change]}
                          </span>
                          <span className="change__name">{change.package}</span>
                          {change.kind === "cask" && <span className="row__id">cask</span>}
                          <span className="change__versions mono">
                            {change.beforeVersion ?? "—"}
                            <span style={{ color: TONE[change.change] }}> → </span>
                            {change.afterVersion ?? "—"}
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </>
  );
}
