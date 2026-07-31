import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { Assessment, Outdated, OpRequest, Service } from "../api/types";
import type { Selection } from "./CatalogView";
import { Tag } from "./Tags";

export function UpdatesList({
  data,
  onSelect,
  onRun,
  busy,
}: {
  data: Outdated | null;
  onSelect: (selection: Selection) => void;
  onRun: (request: OpRequest) => void;
  busy: boolean;
}) {
  const upgradableFormulae = data?.formulae.filter((f) => !f.pinned) ?? [];
  const total = upgradableFormulae.length + (data?.casks.length ?? 0);
  const pinned = data?.formulae.filter((f) => f.pinned) ?? [];

  // Risk and urgency for every pending update. Costs one round trip for the
  // whole list, not one per package.
  const [impact, setImpact] = useState<Map<string, Assessment>>(new Map());
  useEffect(() => {
    if (!data) return;
    api
      .impactAll()
      .then((all) => setImpact(new Map(all.map((a) => [a.package, a]))))
      .catch(() => undefined);
  }, [data]);

  const urgent = [...impact.values()].filter((a) => a.urgency === "high").length;

  return (
    <>
      <header className="header">
        <div style={{ fontSize: "var(--step-1)", fontWeight: 500 }}>
          {total === 0 ? "Everything is up to date" : `${total} available`}
        </div>
        {urgent > 0 && (
          <span className="impact impact--urgent" title="Security fixes waiting">
            {urgent} urgent
          </span>
        )}
        <div className="header__spacer" />
        {upgradableFormulae.length > 0 && (
          <button
            className="btn btn--sm"
            disabled={busy}
            onClick={() => onRun({ action: "upgrade", kind: "formula", targets: [] })}
          >
            Upgrade all formulae
          </button>
        )}
        {(data?.casks.length ?? 0) > 0 && (
          <button
            className="btn btn--sm"
            disabled={busy}
            onClick={() =>
              onRun({
                action: "upgrade",
                kind: "cask",
                targets: data!.casks.map((c) => c.name),
              })
            }
          >
            Upgrade all casks
          </button>
        )}
      </header>

      <div className="scroller">
        {!data ? (
          <>
            <div className="skeleton" />
            <div className="skeleton" />
          </>
        ) : total === 0 && pinned.length === 0 ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ✓
            </div>
            <div>
              Nothing to upgrade.
              <br />
              <span style={{ fontSize: "var(--step--1)", color: "var(--text-faint)" }}>
                Counts match what <span className="mono">brew upgrade</span> would actually change.
              </span>
            </div>
          </div>
        ) : (
          <>
            {upgradableFormulae.map((f) => (
              <UpdateRow
                key={`formula:${f.name}`}
                name={f.name}
                from={f.installed_versions.join(", ")}
                to={f.current_version}
                onSelect={() => onSelect({ kind: "formula", id: f.name })}
                onUpgrade={() =>
                  onRun({ action: "upgrade", kind: "formula", targets: [f.name] })
                }
                busy={busy}
                assessment={impact.get(f.name)}
              />
            ))}
            {data.casks.map((c) => (
              <UpdateRow
                key={`cask:${c.name}`}
                name={c.name}
                badge="cask"
                from={c.installed_versions.join(", ")}
                to={c.current_version}
                onSelect={() => onSelect({ kind: "cask", id: c.name })}
                onUpgrade={() => onRun({ action: "upgrade", kind: "cask", targets: [c.name] })}
                busy={busy}
              />
            ))}
            {pinned.length > 0 && (
              <>
                <div className="listmeta" style={{ paddingTop: "var(--space-5)" }}>
                  Held back by a pin — deliberately not upgraded
                </div>
                {pinned.map((f) => (
                  <UpdateRow
                    key={`pinned:${f.name}`}
                    name={f.name}
                    badge="pinned"
                    from={f.installed_versions.join(", ")}
                    to={f.current_version}
                    onSelect={() => onSelect({ kind: "formula", id: f.name })}
                    busy={busy}
                  />
                ))}
              </>
            )}
          </>
        )}
      </div>
    </>
  );
}

function UpdateRow({
  name,
  from,
  to,
  badge,
  onSelect,
  onUpgrade,
  busy,
  assessment,
}: {
  name: string;
  from: string;
  to: string | null;
  badge?: "cask" | "pinned";
  onSelect: () => void;
  onUpgrade?: () => void;
  busy: boolean;
  assessment?: Assessment;
}) {
  return (
    <div className="row" style={{ alignItems: "flex-start" }}>
      <button
        onClick={onSelect}
        style={{ display: "block", textAlign: "left", minWidth: 0, width: "100%" }}
      >
        <div className="row__head">
          <span className="row__name">{name}</span>
          {badge === "cask" && <span className="row__id">cask</span>}
          {badge === "pinned" && <Tag variant="pinned">pinned</Tag>}
          {assessment && assessment.urgency === "high" && (
            <span className="impact impact--urgent">security</span>
          )}
          {assessment && assessment.risk !== "low" && (
            <span className={`impact impact--${assessment.risk}`}>
              {assessment.risk === "high" ? "high risk" : "some risk"}
            </span>
          )}
        </div>
        <div className="row__desc mono">
          {from} <span style={{ color: "var(--accent)" }}>→</span> {to ?? "?"}
        </div>
        {assessment && assessment.reasons.length > 0 && (
          <ul className="reasons">
            {assessment.reasons.slice(0, 3).map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        )}
      </button>
      {onUpgrade && (
        <button className="btn btn--sm" disabled={busy} onClick={onUpgrade}>
          Upgrade
        </button>
      )}
    </div>
  );
}

export function ServicesList({ services }: { services: Service[] | null }) {
  return (
    <>
      <header className="header">
        <div style={{ fontSize: "var(--step-1)", fontWeight: 500 }}>Background services</div>
      </header>
      <div className="scroller">
        {!services ? (
          <div className="skeleton" />
        ) : services.length === 0 ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ◉
            </div>
            <div>No formulae on this machine provide a background service.</div>
          </div>
        ) : (
          services.map((service) => (
            <div className="row" key={service.name}>
              <div style={{ minWidth: 0 }}>
                <div className="row__head">
                  <span className="row__name">{service.name}</span>
                  {service.status === "started" ? (
                    <Tag variant="installed">running</Tag>
                  ) : service.exit_code && service.exit_code !== 0 ? (
                    <Tag variant="deprecated">exited {service.exit_code}</Tag>
                  ) : (
                    <Tag variant="pinned">{service.status}</Tag>
                  )}
                </div>
                <div className="row__desc mono">{service.file ?? "no service file"}</div>
              </div>
              <span className="row__version">{service.user ?? ""}</span>
            </div>
          ))
        )}
      </div>
    </>
  );
}
