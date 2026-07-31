# own-brew — Design Spec

Date: 2026-07-31
Status: Approved, phase 1 in progress

## Problem

Homebrew has six actively maintained GUI clients (CaskHub, Applite, Cork, Tappie, Taphouse, BrewMate, plus the Tauri-based brew-browser). All of them answer the same question: *what can I install?* None answer the questions that actually cost developers time:

- **"That upgrade broke my toolchain — how do I go back?"** No Homebrew GUI offers rollback. Homebrew itself discourages installing old versions, and `brew switch` was removed.
- **"Don't upgrade that yet."** There is nothing between `brew upgrade` (everything, now) and `brew pin` (never). Workbrew sells update policy, but only to enterprise fleets.
- **"What will this upgrade break?"** Nobody surfaces risk before you apply updates.

own-brew is the Homebrew GUI built around **undo and control** rather than around browsing.

## Positioning

> The Homebrew GUI that can undo an upgrade.

Catalog browsing, install/uninstall and update detection are table stakes — necessary to be usable, never the pitch.

## Feasibility findings (verified 2026-07-31)

These were validated against the live system before committing to the design:

1. **Old bottles are permanently retrievable.** ghcr.io retains a tag per published version — `jq` exposes `1.6-1, 1.7, 1.7.1, 1.7.1-1, 1.8.0, 1.8.1, 1.8.2`. Each tag's OCI index lists per-platform manifests (`1.7.1.arm64_sonoma`, …) whose blob digests are the bottle archives. Rollback targets are therefore fetchable indefinitely, not best-effort.
2. **Old kegs often survive locally.** `brew cleanup` is not automatic; e.g. `python@3.14` had both `3.14.5` and `3.14.6` in the Cellar. Local relink is the fastest rollback path when available.
3. **Bottles are cached on disk.** `~/Library/Caches/Homebrew/downloads` holds `*.bottle.tar.gz` plus `*.bottle_manifest.json` for recent installs — a second offline-capable rollback source.
4. **`brew info --json=v2` is a complete state source.** Per formula it returns `installed[]` (with `runtime_dependencies` pinned to exact versions), `linked_keg`, `pinned`, `outdated`, full `bottle.stable.files.<platform>.{url,sha256}`, dependency lists, and lifecycle fields (`deprecated`, `disabled`, `deprecation_reason`, `deprecation_replacement_formula`).
5. **`brew services list --json`** returns structured service state (`name`, `status`, `user`, `file`, `exit_code`).
6. **`brew vulns`** ships in Homebrew 6.0.11+ (formulae only; casks are not covered).

### Rollback strategy (tiered, degrading gracefully)

| Tier | Source | Cost | Availability |
|---|---|---|---|
| 1 | Old keg still in Cellar → relink | instant, offline | when `brew cleanup` hasn't run |
| 2 | Bottle in Homebrew download cache | fast, offline | recent installs |
| 3 | Fetch bottle blob from ghcr.io by version tag | network | any published version |
| 4 | `brew extract` into a private tap, then install | slow (needs homebrew-core history) | last resort |

The engine picks the cheapest available tier and reports which one it used. Tier availability is computed and shown *before* the user commits to a rollback.

## Architecture

Tauri 2: Rust core owns all privileged work; React owns only presentation.

```
src-tauri/src/
  brew/        # process layer: locate binary, controlled env, stream output, cancel
  model/       # serde types mirroring brew's JSON contracts
  catalog/     # formulae.brew.sh client, on-disk cache, revalidation, search index
  state/       # installed packages, outdated, services — the local truth
  ops/         # install/uninstall/upgrade, streamed to the UI over ipc::Channel
  history/     # SQLite: operation log + pre-op snapshots (rollback substrate)
  commands/    # thin #[tauri::command] wrappers — no logic
src/                 # React 19 + TypeScript
```

**Deliberate decision: no `tauri-plugin-shell`.** Commands are spawned with `tokio::process::Command` from Rust. The webview can only call typed, enumerated Tauri commands, so no code path exists for the frontend to request an arbitrary shell command — the plugin's permission surface is never opened. This also lets the core pin Homebrew's environment (`HOMEBREW_NO_AUTO_UPDATE`, `HOMEBREW_NO_COLOR`, `HOMEBREW_NO_ENV_HINTS`, `HOMEBREW_NO_ANALYTICS`) and own cancellation.

**Streaming:** long operations use `tauri::ipc::Channel`, scoped per invocation, rather than global events — no cross-talk between concurrent operations, and the channel dies with its caller.

**Catalog scale:** ~8k casks + ~16k formulae. The full JSON dumps are fetched once, cached on disk, revalidated with ETag, and searched through a prebuilt in-memory index in Rust. The UI receives paged//filtered slices, never the whole catalog, and renders with virtualization.

## Phasing

**Phase 1 — foundation + table stakes (current):** brew runner, JSON models, catalog client with cache, installed/outdated state, install/uninstall/upgrade with streamed progress, React shell with catalog/search/detail/installed views.

**Phase 2 — the wedge:** operation history + pre-op snapshots, tiered rollback engine, update-policy engine (pin, bake-time, minor-only, never-touch, quiet hours).

**Phase 3 — depth:** upgrade impact preview (release notes + semver distance + build-error analytics), services cockpit (logs, ports, health), `brew vulns` surfacing, dependency graph, disk X-ray, Linux build.

## Non-goals

- Enterprise fleet management (Workbrew's territory).
- A hosted backend or accounts. own-brew is local-first; any future sync is opt-in and additive.
- Reimplementing Homebrew. own-brew drives the real `brew` CLI and never writes to the Cellar directly, so its view can never diverge from the source of truth.

## Verification

Rust: unit tests for the runner (env, arg construction, line streaming, cancellation) and model parsing against fixtures captured from the live `brew` CLI; integration tests that exercise read-only `brew` commands. UI: type-checked build. End-to-end: the app runs against the developer's real Homebrew installation, and every phase-1 claim is checked by performing the operation in the running app.
