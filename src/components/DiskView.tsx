import { useState } from "react";
import type { BrewError, Footprint, OpRequest } from "../api/types";
import { ErrorBanner } from "./ErrorBanner";

export function bytes(value: number): string {
  if (value < 1000) return `${value} B`;
  const units = ["kB", "MB", "GB", "TB"];
  let scaled = value / 1000;
  let unit = 0;
  while (scaled >= 1000 && unit < units.length - 1) {
    scaled /= 1000;
    unit += 1;
  }
  return `${scaled.toFixed(scaled < 10 ? 1 : 0)} ${units[unit]}`;
}

/** Presentational: measuring the Cellar walks gigabytes, so App owns the
 *  result and this only renders it. */
export function DiskView({
  footprint,
  onRun,
  busy,
}: {
  footprint: Footprint | null;
  onRun: (request: OpRequest) => void;
  busy: boolean;
}) {
  const [error] = useState<BrewError | null>(null);
  const [confirming, setConfirming] = useState(false);

  const parts = footprint
    ? [
        { label: "Cellar", value: footprint.cellarBytes, tone: "var(--accent)" },
        { label: "Caskroom", value: footprint.caskroomBytes, tone: "var(--sage)" },
        { label: "Downloads", value: footprint.cacheBytes, tone: "var(--amber)" },
      ]
    : [];

  return (
    <>
      <header className="header">
        <div style={{ fontSize: "var(--step-1)", fontWeight: 500 }}>Disk</div>
        <div className="header__spacer" />
        {footprint && (
          <span className="mono" style={{ fontSize: "var(--step--1)", color: "var(--text-faint)" }}>
            {bytes(footprint.totalBytes)} total
          </span>
        )}
      </header>

      {error && <ErrorBanner error={error} context="Couldn’t measure disk usage" />}

      <div className="scroller">
        {!footprint && !error ? (
          <>
            <div className="skeleton" />
            <div className="skeleton" />
          </>
        ) : footprint ? (
          <div className="disk">
            <div className="disk__bar" role="img" aria-label="Disk usage by area">
              {parts.map((part) => (
                <div
                  key={part.label}
                  className="disk__segment"
                  style={{
                    width: `${(part.value / Math.max(1, footprint.totalBytes)) * 100}%`,
                    background: part.tone,
                  }}
                  title={`${part.label}: ${bytes(part.value)}`}
                />
              ))}
            </div>
            <div className="disk__legend">
              {parts.map((part) => (
                <span key={part.label} className="disk__key">
                  <span className="disk__dot" style={{ background: part.tone }} aria-hidden />
                  {part.label} <span className="mono">{bytes(part.value)}</span>
                </span>
              ))}
            </div>

            <div className="section-title">The price of undo</div>

            {/* The central trade-off of this app, stated rather than buried. */}
            <p className="disk__note">
              own-brew keeps superseded versions on disk so an upgrade can be undone instantly.
              {footprint.superseded.length === 0 ? (
                <> Nothing is being kept right now, so no upgrade can be rolled back instantly.</>
              ) : footprint.superseded.length === 1 ? (
                <>
                  {" "}One is being kept, costing{" "}
                  <strong className="mono">{bytes(footprint.supersededBytes)}</strong>. Reclaiming
                  it is fine — it just means that upgrade can no longer be rolled back instantly.
                </>
              ) : (
                <>
                  {" "}
                  <strong className="mono">{footprint.superseded.length}</strong> are being kept,
                  costing <strong className="mono">{bytes(footprint.supersededBytes)}</strong>.
                  Reclaiming that space is fine — it just means those upgrades can no longer be
                  rolled back instantly.
                </>
              )}
            </p>

            {footprint.superseded.length > 0 && (
              <div className="disk__kegs">
                {footprint.superseded.slice(0, 25).map((keg) => (
                  <div className="restore" key={`${keg.formula}:${keg.version}`}>
                    <div style={{ minWidth: 0 }}>
                      <div className="restore__head">
                        <span className="mono">{keg.formula}</span>
                        <span className="tag tag--rollback">⟲ {keg.version}</span>
                      </div>
                      <div className="restore__note">
                        Restorable now — delete it and this version is gone
                      </div>
                    </div>
                    <span className="restore__hint mono">{bytes(keg.bytes)}</span>
                  </div>
                ))}
                {footprint.superseded.length > 25 && (
                  <p className="disk__note">
                    …and {footprint.superseded.length - 25} more.
                  </p>
                )}
              </div>
            )}

            <div className="section-title">Reclaim</div>
            <p className="disk__note">
              <span className="mono">brew cleanup</span> removes superseded versions and stale
              downloads
              {footprint.cleanupEstimateBytes !== null && (
                <>
                  , freeing about{" "}
                  <strong className="mono">{bytes(footprint.cleanupEstimateBytes)}</strong>
                </>
              )}
              .
            </p>

            {!confirming ? (
              <button
                className="btn"
                disabled={busy}
                onClick={() => setConfirming(true)}
              >
                Reclaim space…
              </button>
            ) : (
              <div className="banner" role="alert">
                <span aria-hidden>⚠</span>
                <div style={{ flex: 1 }}>
                  {footprint.superseded.length === 1 ? (
                    <>
                      This deletes the one superseded version above. The upgrade it would have
                      undone becomes irreversible.
                    </>
                  ) : (
                    <>
                      This deletes the{" "}
                      <strong className="mono">{footprint.superseded.length}</strong> superseded
                      versions above. Any upgrade they would have undone becomes irreversible.
                    </>
                  )}
                  <div style={{ display: "flex", gap: "var(--space-2)", marginTop: 10 }}>
                    <button
                      className="btn btn--sm btn--danger"
                      disabled={busy}
                      onClick={() => {
                        setConfirming(false);
                        onRun({ action: "cleanup", kind: "formula", targets: [] });
                      }}
                    >
                      {footprint.superseded.length === 1
                        ? "Delete it and reclaim"
                        : "Delete them and reclaim"}
                    </button>
                    <button className="btn btn--sm" onClick={() => setConfirming(false)}>
                      Keep my undo
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        ) : null}
      </div>
    </>
  );
}
