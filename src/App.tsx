import { useCallback, useEffect, useMemo, useState } from "react";
import { api, asBrewError } from "./api/client";
import type {
  BrewError,
  CatalogStats,
  Environment,
  InstalledView,
  OpRequest,
  Outdated,
  Service,
} from "./api/types";
import { CatalogView, type Selection } from "./components/CatalogView";
import { Console } from "./components/Console";
import { DetailPanel } from "./components/DetailPanel";
import { ErrorBanner, MissingHomebrew } from "./components/ErrorBanner";
import { InstalledList } from "./components/InstalledView";
import { Rail, type View } from "./components/Rail";
import { ServicesList, UpdatesList } from "./components/UpdatesView";
import { HistoryList } from "./components/HistoryView";
import { useOperations } from "./hooks/useOperations";

type Theme = "dark" | "light";

export default function App() {
  const [view, setView] = useState<View>("discover");
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [stats, setStats] = useState<CatalogStats | null>(null);
  const [installed, setInstalled] = useState<InstalledView | null>(null);
  const [outdated, setOutdated] = useState<Outdated | null>(null);
  const [services, setServices] = useState<Service[] | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [error, setError] = useState<BrewError | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);

  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem("own-brew:theme") as Theme | null) ?? "dark",
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("own-brew:theme", theme);
  }, [theme]);

  /** Re-read everything an operation could have changed.
   *
   *  Settled rather than all-or-nothing: if one of the two calls fails, the
   *  other's result is still worth showing. */
  const refreshLocalState = useCallback(async () => {
    const [installedResult, outdatedResult] = await Promise.allSettled([
      api.installed(),
      api.outdated(),
    ]);

    if (installedResult.status === "fulfilled") setInstalled(installedResult.value);
    if (outdatedResult.status === "fulfilled") setOutdated(outdatedResult.value);

    const failure =
      installedResult.status === "rejected"
        ? installedResult.reason
        : outdatedResult.status === "rejected"
          ? outdatedResult.reason
          : null;
    setError(failure ? asBrewError(failure) : null);

    // Nudge any open detail panel to refetch.
    setRefreshToken((token) => token + 1);
  }, []);

  const onSettled = useCallback(() => {
    void refreshLocalState();
    void api.services().then(setServices).catch(() => undefined);
  }, [refreshLocalState]);

  const { operation, run, cancel, dismiss } = useOperations(onSettled);

  useEffect(() => {
    api
      .environment()
      .then((env) => {
        setEnvironment(env);
        if (!env.brewInstalled) return;
        void refreshLocalState();
        // The catalog is still warming up in the background at this point, so
        // a failure here just means "not ready yet".
        api.catalogStats().then(setStats).catch(() => undefined);
      })
      .catch((e) => setError(asBrewError(e)));
  }, [refreshLocalState]);

  // The catalog preloads on startup; poll briefly until its stats appear.
  useEffect(() => {
    if (stats || !environment?.brewInstalled) return;
    const timer = setInterval(() => {
      api
        .catalogStats()
        .then((s) => setStats(s))
        .catch(() => undefined);
    }, 900);
    return () => clearInterval(timer);
  }, [stats, environment]);

  useEffect(() => {
    if (view === "services" && services === null && environment?.brewInstalled) {
      api.services().then(setServices).catch((e) => setError(asBrewError(e)));
    }
  }, [view, services, environment]);

  /** `kind:id` -> installed version, so catalog rows can show install state. */
  const installedIndex = useMemo(() => {
    const index = new Map<string, string | null>();
    installed?.packages.forEach((pkg) => index.set(`${pkg.kind}:${pkg.id}`, pkg.version));
    return index;
  }, [installed]);

  const busy = operation.running;
  const onRun = useCallback((request: OpRequest) => void run(request), [run]);

  const updateCount = outdated
    ? outdated.formulae.filter((f) => !f.pinned).length + outdated.casks.length
    : undefined;

  if (environment && !environment.brewInstalled) {
    return (
      <>
        <div className="titlebar" data-tauri-drag-region />
        <MissingHomebrew />
      </>
    );
  }

  return (
    <>
      <div className="titlebar" data-tauri-drag-region />
      <div className="app">
        <Rail
          view={view}
          onNavigate={setView}
          counts={{
            installed: installed?.summary.requested,
            updates: updateCount,
            services: services?.filter((s) => s.status === "started").length,
          }}
          stats={stats}
          theme={theme}
          onToggleTheme={() => setTheme(theme === "dark" ? "light" : "dark")}
        />

        <main className="main">
          {error && <ErrorBanner
              error={error}
              context="Couldn’t read local packages"
              onRetry={() => void refreshLocalState()}
            />}

          {view === "discover" && (
            <CatalogView selected={selection} onSelect={setSelection} installed={installedIndex} />
          )}
          {view === "installed" && (
            <InstalledList
              data={installed}
              selected={selection}
              onSelect={setSelection}
              onRun={onRun}
              busy={busy}
            />
          )}
          {view === "updates" && (
            <UpdatesList data={outdated} onSelect={setSelection} onRun={onRun} busy={busy} />
          )}
          {view === "history" && <HistoryList refreshToken={refreshToken} />}
          {view === "services" && <ServicesList services={services} />}
        </main>
      </div>

      {selection && (
        <DetailPanel
          kind={selection.kind}
          id={selection.id}
          onClose={() => setSelection(null)}
          onRun={onRun}
          busy={busy}
          refreshToken={refreshToken}
          onChanged={() => void refreshLocalState()}
        />
      )}

      {(operation.running || operation.finishedAt !== null) && (
        <Console operation={operation} onCancel={() => void cancel()} onDismiss={dismiss} />
      )}
    </>
  );
}
