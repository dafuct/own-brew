import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CatalogStats,
  Detail,
  Environment,
  InstalledView,
  Kind,
  OpEvent,
  OpRequest,
  Outdated,
  Page,
  SearchQuery,
  Service,
  BrewError,
  Operation,
  RollbackCandidate,
  Policy,
  Decision,
} from "./types";

/** Tauri rejects with our serialized Error shape; anything else is a bug. */
export function asBrewError(e: unknown): BrewError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    return e as BrewError;
  }
  return { kind: "unknown", message: String(e), detail: null };
}

export const api = {
  environment: () => invoke<Environment>("environment"),

  catalogStats: () => invoke<CatalogStats>("catalog_stats"),
  catalogReload: () => invoke<CatalogStats>("catalog_reload"),
  search: (query: Partial<SearchQuery>) =>
    invoke<Page>("catalog_search", {
      query: {
        text: "",
        kind: null,
        includeUnavailable: false,
        sort: "relevance",
        limit: 60,
        offset: 0,
        ...query,
      },
    }),

  packageDetail: (kind: Kind, id: string) => invoke<Detail>("package_detail", { kind, id }),

  installed: () => invoke<InstalledView>("installed"),
  outdated: () => invoke<Outdated>("outdated"),
  services: () => invoke<Service[]>("services"),

  /** Runs an operation, streaming events until it settles. */
  run: (request: OpRequest, onEvent: (event: OpEvent) => void) => {
    const channel = new Channel<OpEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("op_run", { request, channel });
  },

  cancel: (id: number) => invoke<void>("op_cancel", { id }),

  history: (limit = 100) => invoke<Operation[]>("history_recent", { limit }),

  rollbackCandidates: (kind: Kind, id: string) =>
    invoke<RollbackCandidate[]>("rollback_candidates", { kind, id }),
  rollbackRestore: (id: string, version: string) =>
    invoke<string>("rollback_restore", { id, version }),

  policies: () => invoke<Policy[]>("policy_list"),
  setPolicy: (policy: Policy) => invoke<void>("policy_set", { policy }),
  decisions: () => invoke<Decision[]>("policy_decisions"),
};
