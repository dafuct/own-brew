# own-brew

**The Homebrew GUI that can undo an upgrade.**

Every Homebrew GUI answers *what can I install?* own-brew also answers the
questions that actually cost time: *what changed, was it safe, and can I take
it back?*

> Status: phase 1. The catalog, install/uninstall/upgrade, pinning and live
> operation streaming all work against real Homebrew. The rollback **engine**
> is phase 2 — what ships today is the substrate it needs: superseded versions
> are preserved and surfaced everywhere as restore candidates.

## Why it exists

macOS has at least six Homebrew GUIs, and they all stop at browsing and
installing. None of them can put a broken upgrade back. Homebrew itself
discourages installing old versions, and `brew switch` was removed years ago —
so "that upgrade broke my toolchain" has no answer beyond hunting down the old
formula by hand.

That gap is the entire product:

- **Rollback (phase 2).** Superseded kegs, the local bottle cache, and ghcr.io
  version tags together make almost any previous version recoverable.
- **Update policy (phase 2).** Nothing today sits between `brew upgrade`
  (everything, now) and `brew pin` (never).
- **Upgrade impact preview (phase 3).** See the risk before applying it.

## What makes rollback possible

Verified against a live installation before the design was committed:

| Source | Cost | Availability |
|---|---|---|
| Superseded keg still in the Cellar | instant, offline | until `brew cleanup` runs |
| Bottle in Homebrew's download cache | fast, offline | recent installs |
| ghcr.io version tag | network | **any published version** |
| `brew extract` into a private tap | slow | last resort |

ghcr.io keeps a tag per published version — `jq` alone exposes `1.6-1` through
`1.8.2` — so rollback targets stay fetchable indefinitely rather than
best-effort.

own-brew also runs Homebrew with `HOMEBREW_NO_INSTALL_CLEANUP=1`, because
Homebrew's periodic cleanup is precisely what destroys the fastest rollback
path. Reclaiming that disk space becomes a deliberate, visible action instead
of a silent one.

## Design

Tauri 2. The Rust core owns everything privileged; React only presents.

```
src-tauri/src/
  brew/      process layer — locate the binary, pin the environment, stream output, cancel
  model/     serde types mirroring brew's JSON contracts
  catalog/   catalog loading, popularity, search ranking
  state/     installed / outdated / services — the local truth
  ops/       install, uninstall, upgrade, pin — streamed over a per-call channel
  commands   thin IPC wrappers
src/         React 19 + TypeScript
```

Three decisions worth knowing:

**No `tauri-plugin-shell`.** Commands are spawned with `tokio::process` from
Rust, so the webview has no path to request an arbitrary shell command — the
plugin's permission surface is never opened. Package ids are validated before
becoming arguments, so an id like `--force` can never be read as a flag.

**The catalog comes from Homebrew's own cache.** Homebrew already keeps signed
JWS dumps on disk and refreshes them itself. Reading those loads 16,000
packages in ~770 ms with no network, and guarantees own-brew shows exactly what
`brew` would install. Downloading from formulae.brew.sh is the fallback.

**Progress streams over a per-invocation channel**, not a global event bus, so
concurrent operations cannot cross-talk.

own-brew never writes to the Cellar itself. It drives the real `brew` CLI, so
its view cannot drift from Homebrew's.

## Develop

```bash
pnpm install
pnpm tauri dev
```

Verify:

```bash
cd src-tauri && cargo test && cargo clippy --all-targets
```

The suite includes integration tests that run against the Homebrew
installation on the machine, because the failure mode that matters most is
Homebrew changing its output — something a mocked test cannot catch. They skip
themselves when Homebrew is absent.

## Requirements

macOS with Homebrew. Linux support is planned and mostly free with Tauri —
Homebrew on Linux is first class, and every SwiftUI competitor is macOS-only.

## License

MIT.
