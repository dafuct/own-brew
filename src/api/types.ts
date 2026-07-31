/** Mirrors of the Rust IPC types. Kept hand-written and narrow on purpose:
 *  these are the only shapes the UI is allowed to assume. */

export type Kind = "formula" | "cask";
export type Sort = "relevance" | "popularity" | "name";

export interface BrewError {
  kind: string;
  message: string;
  detail: string | null;
}

export interface Environment {
  brewInstalled: boolean;
  brewVersion: string | null;
  prefix: string | null;
}

export interface CatalogStats {
  formulae: number;
  casks: number;
  origin: "brew_cache" | "network";
  loadedAt: number;
  hasPopularity: boolean;
}

export interface Entry {
  kind: Kind;
  id: string;
  name: string;
  desc: string | null;
  version: string;
  tap: string;
  homepage: string | null;
  deprecated: boolean;
  disabled: boolean;
  installs_90d: number | null;
}

export interface Page {
  total: number;
  items: Entry[];
}

export interface SearchQuery {
  text: string;
  kind: Kind | null;
  includeUnavailable: boolean;
  sort: Sort;
  limit: number;
  offset: number;
}

export interface InstalledKeg {
  version: string;
  installed_as_dependency: boolean;
  installed_on_request: boolean;
  poured_from_bottle: boolean;
  time: number | null;
}

export interface FormulaDetail {
  kind: "formula";
  name: string;
  full_name: string;
  tap: string | null;
  desc: string | null;
  license: string | null;
  homepage: string | null;
  versions: { stable: string | null; head: string | null; bottle: boolean };
  dependencies: string[];
  build_dependencies: string[];
  conflicts_with: string[];
  keg_only: boolean;
  caveats: string | null;
  installed: InstalledKeg[];
  linked_keg: string | null;
  pinned: boolean;
  outdated: boolean;
  deprecated: boolean;
  deprecation_reason: string | null;
  deprecation_replacement_formula: string | null;
  disabled: boolean;
  disable_reason: string | null;
}

export interface CaskDetail {
  kind: "cask";
  token: string;
  tap: string | null;
  name: string[];
  desc: string | null;
  homepage: string | null;
  url: string | null;
  version: string | null;
  installed: string | null;
  installed_time: number | null;
  outdated: boolean;
  auto_updates: boolean | null;
  depends_on: { cask: string[]; formula: string[]; macos: unknown };
  caveats: string | null;
  artifacts: unknown[];
  deprecated: boolean;
  deprecation_reason: string | null;
  disabled: boolean;
}

export type Detail = FormulaDetail | CaskDetail;

export interface InstalledPackage {
  kind: Kind;
  id: string;
  name: string;
  desc: string | null;
  version: string | null;
  outdated: boolean;
  pinned: boolean;
  installedOnRequest: boolean;
  installedAt: number | null;
  /** Superseded kegs still on disk — instant rollback targets. */
  rollbackTargets: string[];
  selfUpdating: boolean;
}

export interface InstalledSummary {
  formulae: number;
  casks: number;
  requested: number;
  outdated: number;
  pinned: number;
}

export interface InstalledView {
  packages: InstalledPackage[];
  summary: InstalledSummary;
}

export interface OutdatedFormula {
  name: string;
  installed_versions: string[];
  current_version: string | null;
  pinned: boolean;
}

export interface OutdatedCask {
  name: string;
  installed_versions: string[];
  current_version: string | null;
}

export interface Outdated {
  formulae: OutdatedFormula[];
  casks: OutdatedCask[];
}

export interface Service {
  name: string;
  status: string;
  user: string | null;
  file: string | null;
  exit_code: number | null;
}

export type Action = "install" | "uninstall" | "upgrade" | "pin" | "unpin" | "update";

export interface OpRequest {
  action: Action;
  kind: Kind;
  targets: string[];
}

export type OpEvent =
  | { event: "started"; data: { id: number; command: string } }
  | { event: "phase"; data: { id: number; label: string } }
  | { event: "progress"; data: { id: number; percent: number } }
  | { event: "output"; data: { id: number; origin: "stdout" | "stderr"; text: string } }
  | { event: "needsInput"; data: { id: number; text: string } }
  | {
      event: "finished";
      data: { id: number; success: boolean; cancelled: boolean; durationMs: number };
    };
