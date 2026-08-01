import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useRef, useState } from "react";
import { useSearch } from "../hooks/useSearch";
import type { Entry, Kind, Sort } from "../api/types";
import { PopularityBar, Tag } from "./Tags";
import { ErrorBanner } from "./ErrorBanner";

const ROW_HEIGHT = 46;

/** Rows kept rendered beyond the viewport, each side.
 *
 *  Sized for a flick, not a nudge: at 46px a row this is ~1,470px of cover
 *  above and below, so the list stays populated through the frames between
 *  the compositor moving the viewport and React committing new rows. */
const OVERSCAN = 32;

export interface Selection {
  kind: Kind;
  id: string;
}

export function CatalogView({
  selected,
  onSelect,
  installed,
}: {
  selected: Selection | null;
  onSelect: (selection: Selection) => void;
  installed: Map<string, string | null>;
}) {
  const [text, setText] = useState("");
  const [debounced, setDebounced] = useState("");
  const [kind, setKind] = useState<Kind | null>(null);
  const [sort, setSort] = useState<Sort>("relevance");

  // Typing shouldn't fire a search per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(text.trim()), 140);
    return () => clearTimeout(timer);
  }, [text]);

  const search = useSearch(debounced, kind, sort);
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: search.total,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: OVERSCAN,
  });

  const virtualRows = virtualizer.getVirtualItems();
  const firstIndex = virtualRows[0]?.index ?? 0;
  const lastIndex = virtualRows[virtualRows.length - 1]?.index ?? 0;

  // Pull in whichever pages the visible window needs.
  useEffect(() => {
    search.ensureLoaded(firstIndex, lastIndex);
  }, [firstIndex, lastIndex, search]);

  // A new query should start from the top.
  useEffect(() => {
    virtualizer.scrollToOffset(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [debounced, kind, sort]);

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
            placeholder="search 16,000 packages…"
            spellCheck={false}
            autoFocus
            aria-label="Search packages"
          />
        </div>

        <div className="segmented" role="group" aria-label="Package type">
          {(
            [
              [null, "All"],
              ["formula", "Formulae"],
              ["cask", "Casks"],
            ] as const
          ).map(([value, label]) => (
            <button
              key={label}
              aria-pressed={kind === value}
              onClick={() => setKind(value)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="segmented" role="group" aria-label="Sort order">
          {(
            [
              ["relevance", "Best"],
              ["popularity", "Popular"],
              ["name", "A–Z"],
            ] as const
          ).map(([value, label]) => (
            <button key={value} aria-pressed={sort === value} onClick={() => setSort(value)}>
              {label}
            </button>
          ))}
        </div>
      </header>

      {search.error && (
        <ErrorBanner error={search.error} context="Search failed" onRetry={search.reload} />
      )}

      <div className="listmeta">
        {search.loading && search.total === 0
          ? "searching…"
          : `${search.total.toLocaleString()} ${search.total === 1 ? "package" : "packages"}`}
        {debounced && search.total > 0 && <span>· matching “{debounced}”</span>}
      </div>

      <div className="scroller" ref={scrollRef}>
        {search.total === 0 && !search.loading ? (
          <div className="empty">
            <div className="empty__mark" aria-hidden>
              ∅
            </div>
            <div>
              {debounced ? (
                <>
                  Nothing matches “{debounced}”.
                  <br />
                  Try a shorter term, or search by what it does.
                </>
              ) : (
                "The catalog is still loading."
              )}
            </div>
          </div>
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualRows.map((row) => {
              const entry = search.at(row.index);
              return (
                <div
                  key={row.key}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: row.size,
                    transform: `translateY(${row.start}px)`,
                  }}
                >
                  {entry ? (
                    <PackageRow
                      entry={entry}
                      installedVersion={installed.get(`${entry.kind}:${entry.id}`)}
                      selected={selected?.kind === entry.kind && selected.id === entry.id}
                      onSelect={() => onSelect({ kind: entry.kind, id: entry.id })}
                    />
                  ) : (
                    <div className="skeleton" />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}

function PackageRow({
  entry,
  installedVersion,
  selected,
  onSelect,
}: {
  entry: Entry;
  installedVersion: string | null | undefined;
  selected: boolean;
  onSelect: () => void;
}) {
  const isInstalled = installedVersion !== undefined;
  const outdated =
    isInstalled && installedVersion !== null && entry.version !== "" &&
    installedVersion !== entry.version;

  return (
    <button className={`row${selected ? " row--selected" : ""}`} onClick={onSelect}>
      <div style={{ minWidth: 0 }}>
        <div className="row__head">
          <span className="row__name">{entry.name}</span>
          {entry.name !== entry.id && <span className="row__id">{entry.id}</span>}
          {entry.kind === "cask" && <span className="row__id">cask</span>}
          {isInstalled &&
            (outdated ? (
              <Tag variant="outdated" title={`${installedVersion} installed, ${entry.version} available`}>
                update
              </Tag>
            ) : (
              <Tag variant="installed">installed</Tag>
            ))}
          {entry.deprecated && <Tag variant="deprecated">deprecated</Tag>}
          {entry.disabled && <Tag variant="disabled">disabled</Tag>}
        </div>
        <div className="row__desc">{entry.desc ?? "No description"}</div>
      </div>

      <div className="row__right">
        <PopularityBar installs={entry.installs_90d} />
        <span className="row__version">{entry.version || "—"}</span>
      </div>
    </button>
  );
}
