# own-brew

**The Homebrew GUI that can undo an upgrade.**

Every Homebrew GUI answers *what can I install?* own-brew also answers the
questions that actually cost time: *what changed, was it safe, and can I take
it back?*

> Status: phase 2. Rollback works: a superseded version still on disk can be
> restored from the UI, verified end to end against a real installation. The
> operation log and the update-policy engine are in. Restoring a version that
> is *not* on disk is still open — see "What is not done" below.

## Why it exists

macOS has at least six Homebrew GUIs, and they all stop at browsing and
installing. None of them can put a broken upgrade back. Homebrew itself
discourages installing old versions, and `brew switch` was removed years ago —
so "that upgrade broke my toolchain" has no answer beyond hunting down the old
formula by hand.

That gap is the entire product:

- **Rollback.** A superseded keg still on disk restores instantly and offline;
  versions Homebrew publishes separately (`python@3.13`, `node@22`) are offered
  as ordinary installs.
- **Update policy.** `auto`, `bake for N days`, `minor only`, `never` — the
  ground between `brew upgrade` (everything, now) and `brew pin` (never).
- **An operation log** that records what each run actually changed, including
  dependencies you never named.
- **Upgrade impact preview (phase 3).** See the risk before applying it.

## What makes rollback possible

Verified against a live installation before the design was committed:

| Source | Status | Availability |
|---|---|---|
| Superseded keg still in the Cellar | **works** | until `brew cleanup` runs |
| Version Homebrew publishes separately | **works** | `python@3.13`, `node@22`, … |
| Bottle in Homebrew's download cache | discovered, not restorable | recent installs |
| Version seen only in own-brew's history | discovered, not restorable | anything you once ran |

Restoring a local keg drives Homebrew's *own* `Keg#link` through `brew ruby`.
`brew switch` was removed years ago and no CLI verb replaced it, but
re-implementing Homebrew's linking rules — keg-only handling, the `opt` prefix
dependents resolve through, conflict resolution — would be a reliable way to
break a machine. If linking the target fails, the previously linked keg is put
back, so a failed rollback never leaves nothing linked.

### What is not done

Restoring a version that is **not** on disk and has no separately-published
formula. The obvious route was to fetch the historical formula from the commit
ghcr.io records for every bottle — but those commits are unreachable:
Homebrew's merge queue rebases, and `git fetch` of a recorded revision returns
`upload-pack: not our ref`. `brew extract` is the supported alternative and it
requires a full homebrew-core clone, which API-only installs (the modern
default) do not have. Doing this properly means searching homebrew-core's
history through the GitHub API. Until then own-brew *shows* those versions and
says plainly that it cannot restore them, rather than offering a button that
would fail.

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
  history/   SQLite operation log; what each run actually changed
  rollback/  restorable versions, and going back to one
  policy/    per-package update rules and the decisions they produce
  commands   thin IPC wrappers
src/         React 19 + TypeScript
```

Decisions worth knowing:

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

**Rollback targets come from the filesystem, never from `brew info`.** On a
real machine `brew info --json=v2` reported `sdl2-compat 2.32.10` among a
formula's installed kegs while that directory had already been removed —
install receipts outlive the kegs they describe. Offering a restore that
cannot happen would make the headline feature lie, so every candidate is
confirmed against the Cellar.

**History records a diff, not a narration.** Homebrew reports what it did in
prose; own-brew compares the installed set before and after instead, which
stays accurate for packages the user never named, such as dependencies pulled
in by an install.

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
