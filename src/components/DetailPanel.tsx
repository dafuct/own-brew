import { useEffect, useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, CaskDetail, Detail, FormulaDetail, Kind, OpRequest } from "../api/types";
import { ErrorBanner } from "./ErrorBanner";
import { Tag, relativeTime } from "./Tags";
import { PolicySection, RollbackSection } from "./RollbackPanel";

export function DetailPanel({
  kind,
  id,
  onClose,
  onRun,
  busy,
  refreshToken,
  onChanged,
}: {
  kind: Kind;
  id: string;
  onClose: () => void;
  onRun: (request: OpRequest) => void;
  busy: boolean;
  /** Changes after an operation so the panel refetches. */
  refreshToken: number;
  onChanged: () => void;
}) {
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState<BrewError | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
    api
      .packageDetail(kind, id)
      .then((d) => !cancelled && setDetail(d))
      .catch((e) => !cancelled && setError(asBrewError(e)));
    return () => {
      cancelled = true;
    };
  }, [kind, id, refreshToken]);

  return (
    <aside className="detail" aria-label={`${id} details`}>
      <button className="detail__close" onClick={onClose} aria-label="Close details">
        ✕
      </button>

      {error && <ErrorBanner error={error} />}
      {!detail && !error && <div className="skeleton" style={{ margin: "var(--space-5)" }} />}

      {detail?.kind === "formula" && (
        <FormulaBody
          detail={detail}
          onRun={onRun}
          busy={busy}
          refreshToken={refreshToken}
          onChanged={onChanged}
        />
      )}
      {detail?.kind === "cask" && <CaskBody detail={detail} onRun={onRun} busy={busy} />}
    </aside>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="field">
      <div className="field__label">{label}</div>
      <div className="field__value">{children}</div>
    </div>
  );
}

function Chips({ items }: { items: string[] }) {
  if (items.length === 0) return <span style={{ color: "var(--text-faint)" }}>none</span>;
  return (
    <div className="chips">
      {items.map((item) => (
        <span className="chip" key={item}>
          {item}
        </span>
      ))}
    </div>
  );
}

function FormulaBody({
  detail,
  onRun,
  busy,
  refreshToken,
  onChanged,
}: {
  detail: FormulaDetail;
  onRun: (request: OpRequest) => void;
  busy: boolean;
  refreshToken: number;
  onChanged: () => void;
}) {
  const installed = detail.installed.length > 0;
  const active = detail.linked_keg ?? detail.installed.at(-1)?.version ?? null;
  const superseded = detail.installed.filter((keg) => keg.version !== active);

  return (
    <>
      <div className="detail__head">
        <h2 className="detail__title">{detail.name}</h2>
        <div className="detail__id">
          {detail.full_name || detail.name} · {detail.tap ?? "homebrew/core"}
        </div>
        <p className="detail__desc">{detail.desc ?? "No description provided."}</p>
        <div className="detail__tags">
          {installed && <Tag variant="installed">installed {active}</Tag>}
          {detail.outdated && <Tag variant="outdated">update available</Tag>}
          {detail.pinned && <Tag variant="pinned">pinned</Tag>}
          {superseded.length > 0 && (
            <Tag variant="rollback" title="Older versions are still on disk and can be restored">
              ⟲ {superseded.length} older on disk
            </Tag>
          )}
          {detail.deprecated && <Tag variant="deprecated">deprecated</Tag>}
          {detail.disabled && <Tag variant="disabled">disabled</Tag>}
          {detail.keg_only && <Tag variant="pinned">keg-only</Tag>}
        </div>
      </div>

      <div className="detail__body">
        {(detail.deprecation_reason || detail.disable_reason) && (
          <div className="banner" style={{ margin: "0 0 var(--space-4)" }}>
            <span aria-hidden>⚠</span>
            <div>
              {detail.disable_reason
                ? `Disabled: ${detail.disable_reason}`
                : `Deprecated: ${detail.deprecation_reason}`}
              {detail.deprecation_replacement_formula && (
                <div style={{ marginTop: 4 }}>
                  Use <span className="mono">{detail.deprecation_replacement_formula}</span>{" "}
                  instead.
                </div>
              )}
            </div>
          </div>
        )}

        <Field label="Version">
          <span className="mono">{detail.versions.stable ?? "—"}</span>
          {detail.versions.bottle && (
            <span style={{ color: "var(--text-faint)" }}> · precompiled bottle</span>
          )}
        </Field>
        <Field label="License">{detail.license ?? "—"}</Field>
        {detail.homepage && (
          <Field label="Homepage">
            <a href={detail.homepage} target="_blank" rel="noreferrer">
              {detail.homepage}
            </a>
          </Field>
        )}
        <Field label="Depends on">
          <Chips items={detail.dependencies} />
        </Field>
        {detail.conflicts_with.length > 0 && (
          <Field label="Conflicts">
            <Chips items={detail.conflicts_with} />
          </Field>
        )}

        {installed && (
          <>
            <div className="section-title">Versions on disk</div>
            <div className="timeline">
              {[...detail.installed].reverse().map((keg) => {
                const isActive = keg.version === active;
                return (
                  <div className={`keg${isActive ? " keg--active" : ""}`} key={keg.version}>
                    <span className="keg__version">{keg.version}</span>
                    <span className="keg__note">
                      {isActive ? "in use" : "superseded"} · {relativeTime(keg.time)}
                    </span>
                  </div>
                );
              })}
            </div>
          </>
        )}

        <RollbackSection
          kind="formula"
          id={detail.name}
          busy={busy}
          refreshToken={refreshToken}
          onRestored={onChanged}
        />

        <PolicySection
          kind="formula"
          id={detail.name}
          refreshToken={refreshToken}
          onChanged={onChanged}
        />

        {detail.caveats && (
          <>
            <div className="section-title">Caveats</div>
            <div className="caveats">{detail.caveats}</div>
          </>
        )}
      </div>

      <div className="detail__actions">
        {!installed ? (
          <button
            className="btn btn--primary"
            disabled={busy || detail.disabled}
            onClick={() => onRun({ action: "install", kind: "formula", targets: [detail.name] })}
          >
            Install
          </button>
        ) : (
          <>
            {detail.outdated && (
              <button
                className="btn btn--primary"
                disabled={busy}
                onClick={() =>
                  onRun({ action: "upgrade", kind: "formula", targets: [detail.name] })
                }
              >
                Upgrade
              </button>
            )}
            <button
              className="btn"
              disabled={busy}
              onClick={() =>
                onRun({
                  action: detail.pinned ? "unpin" : "pin",
                  kind: "formula",
                  targets: [detail.name],
                })
              }
              title="Pinning holds this formula at its current version"
            >
              {detail.pinned ? "Unpin" : "Pin"}
            </button>
            <button
              className="btn btn--danger"
              disabled={busy}
              onClick={() =>
                onRun({ action: "uninstall", kind: "formula", targets: [detail.name] })
              }
            >
              Uninstall
            </button>
          </>
        )}
      </div>
    </>
  );
}

function CaskBody({
  detail,
  onRun,
  busy,
}: {
  detail: CaskDetail;
  onRun: (request: OpRequest) => void;
  busy: boolean;
}) {
  const installed = detail.installed !== null;
  const title = detail.name.find((n) => n.length > 0) ?? detail.token;

  return (
    <>
      <div className="detail__head">
        <h2 className="detail__title">{title}</h2>
        <div className="detail__id">
          {detail.token} · {detail.tap ?? "homebrew/cask"}
        </div>
        <p className="detail__desc">{detail.desc ?? "No description provided."}</p>
        <div className="detail__tags">
          {installed && <Tag variant="installed">installed {detail.installed}</Tag>}
          {detail.outdated && <Tag variant="outdated">update available</Tag>}
          {detail.auto_updates && (
            <Tag variant="pinned" title="This app updates itself, so Homebrew's version can lag">
              self-updating
            </Tag>
          )}
          {detail.deprecated && <Tag variant="deprecated">deprecated</Tag>}
          {detail.disabled && <Tag variant="disabled">disabled</Tag>}
        </div>
      </div>

      <div className="detail__body">
        <Field label="Version">
          <span className="mono">{detail.version ?? "—"}</span>
        </Field>
        {installed && <Field label="Installed">{relativeTime(detail.installed_time)}</Field>}
        {detail.homepage && (
          <Field label="Homepage">
            <a href={detail.homepage} target="_blank" rel="noreferrer">
              {detail.homepage}
            </a>
          </Field>
        )}
        {detail.depends_on.formula.length + detail.depends_on.cask.length > 0 && (
          <Field label="Requires">
            <Chips items={[...detail.depends_on.formula, ...detail.depends_on.cask]} />
          </Field>
        )}
        {detail.url && (
          <Field label="Source">
            <span className="mono" style={{ fontSize: 10 }}>
              {detail.url}
            </span>
          </Field>
        )}

        {detail.caveats && (
          <>
            <div className="section-title">Caveats</div>
            <div className="caveats">{detail.caveats}</div>
          </>
        )}
      </div>

      <div className="detail__actions">
        {!installed ? (
          <button
            className="btn btn--primary"
            disabled={busy || detail.disabled}
            onClick={() => onRun({ action: "install", kind: "cask", targets: [detail.token] })}
          >
            Install
          </button>
        ) : (
          <>
            {detail.outdated && (
              <button
                className="btn btn--primary"
                disabled={busy}
                onClick={() => onRun({ action: "upgrade", kind: "cask", targets: [detail.token] })}
              >
                Upgrade
              </button>
            )}
            <button
              className="btn btn--danger"
              disabled={busy}
              onClick={() =>
                onRun({ action: "uninstall", kind: "cask", targets: [detail.token] })
              }
            >
              Uninstall
            </button>
          </>
        )}
      </div>
    </>
  );
}
