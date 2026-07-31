import { useEffect, useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, Kind, Policy, RollbackCandidate, Rule } from "../api/types";

const SOURCE_LABEL: Record<RollbackCandidate["source"], string> = {
  local_keg: "on disk",
  download_cache: "cached",
  versioned_formula: "separate formula",
  history_only: "from history",
};

/** Restoring a previous version — the reason own-brew exists. */
export function RollbackSection({
  kind,
  id,
  busy,
  onRestored,
  refreshToken,
}: {
  kind: Kind;
  id: string;
  busy: boolean;
  onRestored: () => void;
  refreshToken: number;
}) {
  const [candidates, setCandidates] = useState<RollbackCandidate[] | null>(null);
  const [error, setError] = useState<BrewError | null>(null);
  const [working, setWorking] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCandidates(null);
    api
      .rollbackCandidates(kind, id)
      .then((found) => !cancelled && setCandidates(found))
      .catch((e) => !cancelled && setError(asBrewError(e)));
    return () => {
      cancelled = true;
    };
  }, [kind, id, refreshToken]);

  const restore = async (version: string) => {
    setWorking(version);
    setError(null);
    try {
      await api.rollbackRestore(id, version);
      onRestored();
    } catch (e) {
      setError(asBrewError(e));
    } finally {
      setWorking(null);
    }
  };

  if (!candidates || candidates.length === 0) return null;

  return (
    <>
      <div className="section-title">Go back to</div>

      {error && (
        <div className="banner" style={{ margin: "0 0 var(--space-3)" }}>
          <span aria-hidden>⚠</span>
          <div>
            {error.message}
            {error.detail && <div className="banner__detail">{error.detail}</div>}
          </div>
        </div>
      )}

      {candidates.map((candidate) => (
        <div className="restore" key={`${candidate.source}:${candidate.version}`}>
          <div style={{ minWidth: 0 }}>
            <div className="restore__head">
              <span className="mono">{candidate.version}</span>
              <span className="tag tag--pinned">{SOURCE_LABEL[candidate.source]}</span>
            </div>
            <div className="restore__note">{candidate.note}</div>
          </div>

          {candidate.source === "local_keg" ? (
            <button
              className="btn btn--sm btn--primary"
              disabled={busy || working !== null}
              onClick={() => void restore(candidate.version)}
            >
              {working === candidate.version ? "Restoring…" : "Restore"}
            </button>
          ) : candidate.source === "versioned_formula" ? (
            <span className="restore__hint mono">{candidate.formula}</span>
          ) : (
            <span className="restore__hint">unavailable</span>
          )}
        </div>
      ))}
    </>
  );
}

const RULES: { rule: Rule; label: string; hint: string }[] = [
  { rule: "auto", label: "Auto", hint: "Upgrade whenever an update appears" },
  { rule: "bake", label: "Bake", hint: "Wait a few days before taking a new release" },
  { rule: "minor_only", label: "Minor only", hint: "Skip major version changes" },
  { rule: "never", label: "Never", hint: "Leave this package exactly where it is" },
];

/** The ground between "upgrade everything" and "pin forever". */
export function PolicySection({
  kind,
  id,
  refreshToken,
  onChanged,
}: {
  kind: Kind;
  id: string;
  refreshToken: number;
  onChanged: () => void;
}) {
  const [policy, setPolicy] = useState<Policy | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .policies()
      .then((all) => {
        if (cancelled) return;
        const found = all.find((p) => p.kind === kind && p.package === id);
        setPolicy(found ?? { kind, package: id, rule: "auto", bakeDays: null, note: null });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [kind, id, refreshToken]);

  if (!policy) return null;

  const apply = async (next: Partial<Policy>) => {
    const updated: Policy = { ...policy, ...next };
    setPolicy(updated);
    try {
      await api.setPolicy(updated);
      onChanged();
    } catch {
      /* the next refresh re-reads the stored value */
    }
  };

  const active = RULES.find((r) => r.rule === policy.rule) ?? RULES[0];

  return (
    <>
      <div className="section-title">Update policy</div>
      <div className="segmented" style={{ width: "fit-content" }} role="group">
        {RULES.map(({ rule, label }) => (
          <button
            key={rule}
            aria-pressed={policy.rule === rule}
            onClick={() => void apply({ rule, bakeDays: rule === "bake" ? policy.bakeDays ?? 7 : null })}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="restore__note" style={{ marginTop: 6 }}>
        {active.hint}
      </div>

      {policy.rule === "bake" && (
        <label className="bake">
          Wait
          <input
            type="number"
            min={1}
            max={90}
            value={policy.bakeDays ?? 7}
            onChange={(e) =>
              void apply({ bakeDays: Math.max(1, Math.min(90, Number(e.target.value) || 1)) })
            }
          />
          days before upgrading
        </label>
      )}
    </>
  );
}
