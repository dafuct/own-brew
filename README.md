# own-brew

**The Homebrew GUI that can undo an upgrade.**

Every Homebrew GUI answers *what can I install?* own-brew also answers the
questions that actually cost time: *what changed, was it safe, and can I take
it back?*

> Status: phase 4. Any version Homebrew ever published can now be rolled back
> to, whether or not anything for it survives locally — verified end to end by
> taking `jq` from 1.8.2 down to 1.8.1 and back.

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
- **Upgrade impact preview.** Two independent axes, never one blended score:
  *risk* (how many installed packages depend on this, how far the version
  moves, how often it fails to build for others) and *urgency* (known
  vulnerabilities in the version you are running). Collapsing them would hide
  the case that matters most — a frightening upgrade you should do anyway.
- **Known vulnerabilities**, from Homebrew's own `brew vulns`, with its
  coverage gaps stated rather than glossed over.
- **A disk view** that names the trade-off: superseded versions *are* the undo
  capability, and reclaiming that space removes it.

## What makes rollback possible

Verified against a live installation before the design was committed:

| Source | Cost | Availability |
|---|---|---|
| Superseded keg still in the Cellar | instant, offline, nothing replaced | until `brew cleanup` runs |
| Version Homebrew publishes separately | ordinary install | `python@3.13`, `node@22`, … |
| Recovered from homebrew-core history | a download; replaces the installed version | **any published version** |

Restoring a local keg drives Homebrew's *own* `Keg#link` through `brew ruby`.
`brew switch` was removed years ago and no CLI verb replaced it, but
re-implementing Homebrew's linking rules — keg-only handling, the `opt` prefix
dependents resolve through, conflict resolution — would be a reliable way to
break a machine. If linking the target fails, the previously linked keg is put
back, so a failed rollback never leaves nothing linked.

### Recovering a version that is gone

The obvious route is a dead end. Every bottle records the homebrew-core commit
that built it, but those commits are unreachable — the merge queue rebases, so
`git fetch` of a recorded revision answers `upload-pack: not our ref`. And
`brew extract`, the supported alternative, needs a full homebrew-core clone
that API-only installs no longer have.

What works is asking GitHub which commits touched the formula file. Those SHAs
*are* reachable, and a formula's whole history is a few dozen commits. The file
is fetched, written into own-brew's own tap, and Homebrew is asked to confirm
the version before anything is installed.

Two constraints shape the result, both discovered the hard way:

- **The file keeps its original name.** Naming it `jq@1.8.1` — the shape
  `brew extract` produces — makes Homebrew build the bottle URL from the
  formula name via `image_formula_name`, which maps `@` to `/`. It then looks
  for the bottle at `homebrew/core/jq/1.8.1` and gets a 404.
- **A recovery replaces the installed version.** Homebrew refuses to hold two
  formulae of the same name from different taps, so the current version must
  be uninstalled first. The bottle is therefore downloaded *before* anything
  is removed, and the original is reinstalled if the install still fails — a
  failed recovery must never leave you with no package at all.

Recovery is refused outright when other installed packages depend on the one
being rolled back, since replacing it would break them.

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
  security/  known vulnerabilities, via brew vulns
  impact/    risk and urgency for a pending upgrade
  disk/      what Homebrew costs, and what reclaiming would give back
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

**Blast radius is computed by inverting the installed graph once.** Asking
`brew uses --installed` per package costs a process each and would take the
better part of a minute for a full update list. Inverting the
`runtime_dependencies` already present in `brew info --json=v2 --installed`
gives the same answer for everything at no extra cost — verified to match
`brew uses` exactly, and guarded by a test that keeps checking. Inverting the
plain `dependencies` field instead would silently under-report, finding 9
dependents for `openssl@3` where Homebrew reports 24.

**A clean vulnerability report is not a clean bill of health.** `brew vulns`
covers formulae only — casks, which is most people's GUI software, are not
checked at all — and skips formulae with no derivable upstream repository. The
UI says so on the page rather than showing a reassuring tick.

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
