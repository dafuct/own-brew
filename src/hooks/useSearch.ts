import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, Entry, Kind, Sort } from "../api/types";

const PAGE = 100;

export interface SearchState {
  total: number;
  /** Sparse: an index is `undefined` until its page has arrived. */
  at: (index: number) => Entry | undefined;
  ensureLoaded: (start: number, end: number) => void;
  loading: boolean;
  error: BrewError | null;
  reload: () => void;
}

/**
 * Windowed catalog search.
 *
 * The backend holds all ~16,000 entries, but shipping them over IPC would be
 * wasteful, so pages are pulled in as the virtualizer scrolls into them.
 */
export function useSearch(text: string, kind: Kind | null, sort: Sort): SearchState {
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<BrewError | null>(null);
  /** Bumped whenever loaded data changes, to re-render consumers. */
  const [, setVersion] = useState(0);

  const pages = useRef(new Map<number, Entry[]>());
  const inFlight = useRef(new Set<number>());
  /** Guards against a slow response for a stale query overwriting a fresh one. */
  const generation = useRef(0);

  const queryKey = useMemo(() => JSON.stringify({ text, kind, sort }), [text, kind, sort]);

  const loadPage = useCallback(
    async (page: number) => {
      if (pages.current.has(page) || inFlight.current.has(page)) return;
      inFlight.current.add(page);
      const mine = generation.current;

      try {
        const result = await api.search({
          text,
          kind,
          sort,
          limit: PAGE,
          offset: page * PAGE,
        });
        if (mine !== generation.current) return; // query changed underneath us
        pages.current.set(page, result.items);
        setTotal(result.total);
        setVersion((v) => v + 1);
      } catch (e) {
        if (mine === generation.current) setError(asBrewError(e));
      } finally {
        inFlight.current.delete(page);
        if (mine === generation.current) setLoading(false);
      }
    },
    [text, kind, sort],
  );

  const reset = useCallback(() => {
    generation.current += 1;
    pages.current.clear();
    inFlight.current.clear();
    setError(null);
    setLoading(true);
    setTotal(0);
    setVersion((v) => v + 1);
    void loadPage(0);
  }, [loadPage]);

  useEffect(reset, [queryKey, reset]);

  const at = useCallback((index: number) => {
    const page = pages.current.get(Math.floor(index / PAGE));
    return page?.[index % PAGE];
  }, []);

  const ensureLoaded = useCallback(
    (start: number, end: number) => {
      const first = Math.floor(Math.max(0, start) / PAGE);
      const last = Math.floor(Math.max(0, end) / PAGE);
      for (let page = first; page <= last; page += 1) void loadPage(page);
    },
    [loadPage],
  );

  return { total, at, ensureLoaded, loading, error, reload: reset };
}
