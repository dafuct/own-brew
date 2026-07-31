import type { BrewError } from "../api/types";

export function ErrorBanner({
  error,
  context,
  onRetry,
}: {
  error: BrewError;
  /** What was being attempted, so two banners are never indistinguishable. */
  context?: string;
  onRetry?: () => void;
}) {
  return (
    <div className="banner" role="alert">
      <span aria-hidden style={{ color: "var(--rust)" }}>
        ⚠
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div>
          {context && <strong>{context}: </strong>}
          {error.message}
        </div>
        {error.detail && <div className="banner__detail">{error.detail}</div>}
      </div>
      {onRetry && (
        <button className="btn btn--sm" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

/** Shown instead of the app when Homebrew itself is missing. */
export function MissingHomebrew() {
  return (
    <div className="empty" style={{ height: "100%", alignContent: "center" }}>
      <div className="empty__mark" aria-hidden>
        ◑
      </div>
      <h2 style={{ margin: 0, fontSize: "var(--step-2)" }}>Homebrew isn’t installed</h2>
      <p style={{ maxWidth: 380, lineHeight: 1.6, margin: 0 }}>
        own-brew drives the real <span className="mono">brew</span> command, so Homebrew needs to
        be installed first. Paste this into a terminal, then reopen own-brew:
      </p>
      <code
        className="caveats"
        style={{ userSelect: "all", textAlign: "left", maxWidth: 440 }}
      >
        /bin/bash -c "$(curl -fsSL
        https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
      </code>
    </div>
  );
}
