import type { CatalogStats } from "../api/types";

export type View = "discover" | "installed" | "updates" | "services";

const NAV: { view: View; glyph: string; label: string }[] = [
  { view: "discover", glyph: "⌕", label: "Discover" },
  { view: "installed", glyph: "▤", label: "Installed" },
  { view: "updates", glyph: "↑", label: "Updates" },
  { view: "services", glyph: "◉", label: "Services" },
];

export function Rail({
  view,
  onNavigate,
  counts,
  stats,
  theme,
  onToggleTheme,
}: {
  view: View;
  onNavigate: (view: View) => void;
  counts: Partial<Record<View, number>>;
  stats: CatalogStats | null;
  theme: "dark" | "light";
  onToggleTheme: () => void;
}) {
  return (
    <nav className="rail">
      <div className="rail__brand">
        <span className="rail__mark">◑</span>
        <span className="rail__wordmark">own·brew</span>
      </div>

      {NAV.map(({ view: target, glyph, label }) => {
        const count = counts[target];
        return (
          <button
            key={target}
            className={`navitem${view === target ? " navitem--active" : ""}`}
            onClick={() => onNavigate(target)}
            aria-current={view === target ? "page" : undefined}
          >
            <span className="navitem__glyph" aria-hidden>
              {glyph}
            </span>
            <span className="navitem__label">{label}</span>
            {count !== undefined && count > 0 && (
              <span
                className={`navitem__count${target === "updates" ? " navitem__count--alert" : ""}`}
              >
                {count}
              </span>
            )}
          </button>
        );
      })}

      <div className="rail__footer">
        {stats ? (
          <div>
            <span className="mono">{stats.formulae.toLocaleString()}</span> formulae ·{" "}
            <span className="mono">{stats.casks.toLocaleString()}</span> casks
            <br />
            <span style={{ opacity: 0.75 }}>
              {stats.origin === "brew_cache" ? "from local cache" : "downloaded"}
            </span>
          </div>
        ) : (
          <div>loading catalog…</div>
        )}
        <button onClick={onToggleTheme} style={{ marginTop: 6 }}>
          {theme === "dark" ? "light theme" : "dark theme"}
        </button>
      </div>
    </nav>
  );
}
