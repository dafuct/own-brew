import { useMemo, useState } from "react";
import type { InstalledView as View, OpRequest } from "../api/types";
import type { Selection } from "./CatalogView";
import { Tag, relativeTime } from "./Tags";

type Filter = "all" | "requested" | "outdated" | "rollback";

export function InstalledList({
  data,
  selected,
  onSelect,
  onRun,
  busy,
}: {
  data: View | null;
  selected: Selection | null;
  onSelect: (selection: Selection) => void;
  onRun: (request: OpRequest) => void;
  busy: boolean;
}) {
  const [filter, setFilter] = useState<Filter>("requested");
  const [text, setText] = useState("");

  const packages = useMemo(() => {
    if (!data) return [];
    const needle = text.trim().toLowerCase();
    return data.packages.filter((pkg) => {
      if (filter === "requested" && !pkg.installedOnRequest) return false;
      if (filter === "outdated" && !pkg.outdated) return false;
      if (filter === "rollback" && pkg.rollbackTargets.length === 0) return false;
      if (needle && !`${pkg.name} ${pkg.id}`.toLowerCase().includes(needle)) return false;
      return true;
    });
  }, [data, filter, text]);

  const rollbackable = data?.packages.filter((p) => p.rollbackTargets.length > 0).length ?? 0;

  return (
    <>
      <header className="header">
        <div className="search">
          <span className="search__prompt" aria-hidden>
            ⌕
          </span>
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="filter installed…"
            spellCheck={false}
            aria-label="Filter installed packages"
          />
        </div>

        <div className="segmented" role="group" aria-label="Filter">
          {(
            [
              ["requested", "Requested"],
              ["all", "All"],
              ["outdated", "Outdated"],
              ["rollback", "Undoable"],
            ] as const
          ).map(([value, label]) => (
            <button key={value} aria-pressed={filter === value} onClick={() => setFilter(value)}>
              {label}
              {value === "rollback" && rollbackable > 0 && (
                <span className="mono" style={{ marginLeft: 5, opacity: 0.7 }}>
                  {rollbackable}
                </span>
              )}
            </button>
          ))}
        </div>

        <div className="header__spacer" />

        <button
          className="btn btn--sm"
          disabled={busy}
          onClick={() => onRun({ action: "update", kind: "formula", targets: [] })}
          title="Refresh Homebrew and its taps"
        >
          Refresh Homebrew
        </button>
      </header>

      {data && (
        <div className="listmeta">
          <span className="mono">{data.summary.requested}</span> requested ·{" "}
          <span className="mono">{data.summary.formulae}</span> formulae ·{" "}
          <span className="mono">{data.summary.casks}</span> casks
          {data.summary.outdated > 0 && (
            <span style={{ color: "var(--amber)" }}>
              · <span className="mono">{data.summary.outdated}</span> outdated
            </span>
          )}
          {rollbackable > 0 && (
            <span style={{ color: "var(--accent)" }}>
              · <span className="mono">{rollbackable}</span> with older versions kept
            </span>
          )}
        </div>
      )}

      <div className="scroller">
        {!data ? (
          <>
            <div className="skeleton" />
            <div className="skeleton" />
            <div className="skeleton" />
          </>
        ) : packages.length === 0 ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ∅
            </div>
            <div>Nothing here yet.</div>
          </div>
        ) : (
          packages.map((pkg) => (
            <button
              key={`${pkg.kind}:${pkg.id}`}
              className={`row${
                selected?.kind === pkg.kind && selected.id === pkg.id ? " row--selected" : ""
              }`}
              onClick={() => onSelect({ kind: pkg.kind, id: pkg.id })}
            >
              <div style={{ minWidth: 0 }}>
                <div className="row__head">
                  <span className="row__name">{pkg.name}</span>
                  {pkg.kind === "cask" && <span className="row__id">cask</span>}
                  {!pkg.installedOnRequest && (
                    <span className="row__id" title="Installed because something else needs it">
                      dependency
                    </span>
                  )}
                  {pkg.outdated && <Tag variant="outdated">update</Tag>}
                  {pkg.pinned && <Tag variant="pinned">pinned</Tag>}
                  {pkg.selfUpdating && <Tag variant="pinned">self-updating</Tag>}
                  {pkg.rollbackTargets.length > 0 && (
                    <Tag
                      variant="rollback"
                      title={`Older versions kept on disk: ${pkg.rollbackTargets.join(", ")}`}
                    >
                      ⟲ {pkg.rollbackTargets.length}
                    </Tag>
                  )}
                </div>
                <div className="row__desc">{pkg.desc ?? "No description"}</div>
              </div>

              <div className="row__right">
                <span className="row__version" title="Installed">
                  {relativeTime(pkg.installedAt)}
                </span>
                <span className="row__version">{pkg.version ?? "—"}</span>
              </div>
            </button>
          ))
        )}
      </div>
    </>
  );
}
