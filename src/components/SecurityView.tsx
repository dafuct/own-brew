import { useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, SecurityReport, Severity } from "../api/types";
import { ErrorBanner } from "./ErrorBanner";

const TONE: Record<Severity, string> = {
  CRITICAL: "var(--rust)",
  HIGH: "var(--amber)",
  MEDIUM: "var(--accent)",
  LOW: "var(--text-muted)",
  UNKNOWN: "var(--text-faint)",
};

/** Presentational: the scan itself is owned by App so it survives tab
 *  switches instead of re-running `brew vulns` on every remount. */
export function SecurityView({
  report,
  onRescan,
}: {
  report: SecurityReport | null;
  onRescan: () => void;
}) {
  const [error, setError] = useState<BrewError | null>(null);
  const [scanning, setScanning] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const scan = () => {
    setScanning(true);
    setError(null);
    api
      .securityScan()
      .then(() => onRescan())
      .catch((e) => setError(asBrewError(e)))
      .finally(() => setScanning(false));
  };

  return (
    <>
      <header className="header">
        <div style={{ fontSize: "var(--step-1)", fontWeight: 500 }}>Known vulnerabilities</div>
        <div className="header__spacer" />
        <button className="btn btn--sm" disabled={scanning} onClick={scan}>
          {scanning ? "Scanning…" : "Rescan"}
        </button>
      </header>

      {error && <ErrorBanner error={error} context="Scan failed" onRetry={scan} />}

      {report && (
        <div className="listmeta">
          {report.total === 0 ? (
            "No known vulnerabilities in the formulae that could be checked"
          ) : (
            <>
              <span className="mono">{report.total}</span> findings across{" "}
              <span className="mono">{report.packages.length}</span> packages
              {report.critical > 0 && (
                <span style={{ color: TONE.CRITICAL }}>
                  · <span className="mono">{report.critical}</span> critical
                </span>
              )}
              {report.high > 0 && (
                <span style={{ color: TONE.HIGH }}>
                  · <span className="mono">{report.high}</span> high
                </span>
              )}
            </>
          )}
        </div>
      )}

      {/* A clean report is not proof of safety, and saying so is the honest
          thing to do rather than showing a reassuring green tick. */}
      <div className="banner banner--info" role="note">
        <span aria-hidden>ⓘ</span>
        <div>
          Homebrew’s scanner checks <strong>formulae only</strong> — casks (the GUI apps in your
          Applications folder) are not covered at all, and formulae with no derivable upstream
          repository are skipped. An empty result means nothing was found, not that nothing is
          there.
        </div>
      </div>

      <div className="scroller">
        {!report && !error ? (
          <>
            <div className="skeleton" />
            <div className="skeleton" />
          </>
        ) : report?.total === 0 ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ✓
            </div>
            <div>Nothing found in the packages that could be checked.</div>
          </div>
        ) : (
          report?.packages.map((pkg) => {
            const open = expanded === pkg.formula;
            const worst = pkg.vulnerabilities.reduce<Severity>(
              (acc, v) =>
                order(v.severity) > order(acc) ? v.severity : acc,
              "UNKNOWN",
            );
            return (
              <div className="op" key={pkg.formula}>
                <button
                  className="op__head"
                  onClick={() => setExpanded(open ? null : pkg.formula)}
                >
                  <span className="op__status" style={{ color: TONE[worst] }} aria-hidden>
                    ●
                  </span>
                  <span className="op__command mono">
                    {pkg.formula} {pkg.version ?? ""}
                  </span>
                  <span
                    className="op__count mono"
                    style={{ color: TONE[worst], background: "transparent" }}
                  >
                    {pkg.vulnerabilities.length} · {worst.toLowerCase()}
                  </span>
                </button>

                {open && (
                  <div className="op__body">
                    {pkg.vulnerabilities.map((v) => (
                      <div className="cve" key={v.id}>
                        <span className="cve__severity mono" style={{ color: TONE[v.severity] }}>
                          {v.severity}
                        </span>
                        <span className="cve__id mono">{v.id}</span>
                        <span className="cve__summary">
                          {v.summary ?? "No description published"}
                        </span>
                      </div>
                    ))}
                    {pkg.repoUrl && (
                      <div className="restore__note" style={{ marginTop: 6 }}>
                        Upstream: <span className="mono">{pkg.repoUrl}</span>
                      </div>
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

function order(severity: Severity): number {
  return { UNKNOWN: 0, LOW: 1, MEDIUM: 2, HIGH: 3, CRITICAL: 4 }[severity];
}
