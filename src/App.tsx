import { useCallback, useEffect, useMemo, useState } from "react";
import { api, asBrewError } from "./api/client";
import type {
  Assessment,
  BrewError,
  CatalogStats,
  Environment,
  Footprint,
  InstalledView,
  OpRequest,
  Outdated,
  SecurityReport,
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
import { SecurityView } from "./components/SecurityView";
import { DiskView } from "./components/DiskView";
import { useOperations } from "./hooks/useOperations";

type Theme = "dark" | "light";

export default function App() {
  const [view, setView] = useState<View>("discover");
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [stats, setStats] = useState<CatalogStats | null>(null);
  const [installed, setInstalled] = useState<InstalledView | null>(null);
  const [outdated, setOutdated] = useState<Outdated | null>(null);
  const [services, setServices] = useState<Service[] | null>(null);
  // Security, disk and impact each cost seconds of real work (brew vulns, a
  // 2 GB directory walk, a vulnerability scan plus two brew calls). Holding
  // them here means switching tabs is instant instead of redoing that work on
  // every remount. `stale` marks them for refetch after an operation without
  // paying the cost until the user actually looks.
  const [security, setSecurity] = useState<SecurityReport | null>(null);
  const [footprint, setFootprint] = useState<Footprint | null>(null);
  const [impact, setImpact] = useState<Assessment[] | null>(null);
  const [stale, setStale] = useState({ security: true, disk: true, impact: true });
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
    // Mark, don't fetch: the user may never open these tabs.
    setStale({ security: true, disk: true, impact: true });
  }, [refreshLocalState]);

  const { operation, run, recover, cancel, dismiss } = useOperations(onSettled);

  useEffect(() => {
    api
      .environment()
      .then((env) => {
        setEnvironment(env);
        if (!env.brewInstalled) return;
        void refreshLocalState();
        // The catalog is still warming up in the background at this point, so
        // a failure here just means "not ready yet".
        // Resolves once the catalog has finished loading; no polling needed.
        api.catalogStats().then(setStats).catch(() => undefined);
      })
      .catch((e) => setError(asBrewError(e)));
  }, [refreshLocalState]);

  useEffect(() => {
    if (!environment?.brewInstalled) return;

    if (view === "services" && services === null) {
      api.services().then(setServices).catch((e) => setError(asBrewError(e)));
    }
    if (view === "security" && stale.security) {
      setStale((s) => ({ ...s, security: false }));
      api.securityScan().then(setSecurity).catch(() => undefined);
    }
    if (view === "disk" && stale.disk) {
      setStale((s) => ({ ...s, disk: false }));
      setFootprint(null);
      api.diskFootprint().then(setFootprint).catch(() => undefined);
    }
    if (view === "updates" && stale.impact) {
      setStale((s) => ({ ...s, impact: false }));
      api.impactAll().then(setImpact).catch(() => undefined);
    }
  }, [view, services, environment, stale]);

  /** `kind:id` -> installed version, so catalog rows can show install state. */
  const installedIndex = useMemo(() => {
    const index = new Map<string, string | null>();
    installed?.packages.forEach((pkg) => index.set(`${pkg.kind}:${pkg.id}`, pkg.version));
    return index;
  }, [installed]);

  const busy = operation.running;
  const onRun = useCallback((request: OpRequest) => void run(request), [run]);

  const onRecover = useCallback(
    (id: string, version: string) => void recover(id, version),
    [recover],
  );

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
            security: security ? security.critical + security.high : undefined,
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
            <UpdatesList
              data={outdated}
              assessments={impact}
              onSelect={setSelection}
              onRun={onRun}
              busy={busy}
            />
          )}
          {view === "security" && (
            <SecurityView
              report={security}
              onRescan={() => setStale((st) => ({ ...st, security: true }))}
            />
          )}
          {view === "history" && <HistoryList refreshToken={refreshToken} />}
          {view === "disk" && <DiskView footprint={footprint} onRun={onRun} busy={busy} />}
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
          onRecover={onRecover}
        />
      )}

      {(operation.running || operation.finishedAt !== null) && (
        <Console operation={operation} onCancel={() => void cancel()} onDismiss={dismiss} />
      )}
    </>
  );
}
