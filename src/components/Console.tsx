import { useEffect, useRef, useState } from "react";
import type { Operation } from "../hooks/useOperations";

/**
 * The live operation drawer.
 *
 * Homebrew's own output is the source of truth about what is happening, so it
 * is shown verbatim rather than hidden behind an indeterminate spinner.
 */
export function Console({
  operation,
  onCancel,
  onDismiss,
}: {
  operation: Operation;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const logRef = useRef<HTMLDivElement>(null);
  const pinnedToBottom = useRef(true);

  // Follow the tail, unless the user has scrolled up to read something.
  useEffect(() => {
    const log = logRef.current;
    if (!log || !pinnedToBottom.current) return;
    log.scrollTop = log.scrollHeight;
  }, [operation.lines]);

  const onScroll = () => {
    const log = logRef.current;
    if (!log) return;
    pinnedToBottom.current = log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  };

  const tone =
    operation.phase === "Failed"
      ? "var(--rust)"
      : operation.phase === "Done"
        ? "var(--sage)"
        : "var(--text)";

  return (
    <section className="console" aria-label="Operation progress">
      <div className="console__bar">
        {operation.running ? (
          <span className="console__spinner" aria-hidden />
        ) : (
          <span aria-hidden style={{ color: tone, width: 9, textAlign: "center" }}>
            {operation.phase === "Failed" ? "✕" : "✓"}
          </span>
        )}

        <div style={{ minWidth: 0, flex: "0 1 auto" }}>
          <div className="console__phase" style={{ color: tone }}>
            {operation.phase || "Working"}
          </div>
          <div className="console__cmd">{operation.command}</div>
        </div>

        {operation.percent !== null && (
          <div className="progress" role="progressbar" aria-valuenow={operation.percent}>
            <div className="progress__fill" style={{ width: `${operation.percent}%` }} />
          </div>
        )}

        <div style={{ flex: 1 }} />

        <button className="btn btn--sm" onClick={() => setExpanded((v) => !v)}>
          {expanded ? "Hide log" : "Show log"}
        </button>
        {operation.running ? (
          <button className="btn btn--sm btn--danger" onClick={onCancel}>
            Cancel
          </button>
        ) : (
          <button className="btn btn--sm" onClick={onDismiss}>
            Close
          </button>
        )}
      </div>

      {operation.blockedOn && (
        <div className="banner" role="alert" style={{ margin: "0 var(--space-5) var(--space-3)" }}>
          <span aria-hidden>⚠</span>
          <div>
            Homebrew is asking for input and can’t be answered from here:
            <div className="banner__detail">{operation.blockedOn}</div>
            Cancel and run the command in a terminal to continue.
          </div>
        </div>
      )}

      {expanded && (
        <div className="console__log" ref={logRef} onScroll={onScroll}>
          {operation.lines.length === 0 ? (
            <div className="console__line" style={{ opacity: 0.6 }}>
              waiting for output…
            </div>
          ) : (
            operation.lines.map((line) => (
              <div
                key={line.key}
                className={`console__line${
                  line.tone === "err"
                    ? " console__line--stderr"
                    : line.tone === "phase"
                      ? " console__line--phase"
                      : ""
                }`}
              >
                {line.tone === "phase" ? `==> ${line.text}` : line.text}
              </div>
            ))
          )}
        </div>
      )}
    </section>
  );
}
