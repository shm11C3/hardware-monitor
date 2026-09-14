# Local Build Cache Across Git Worktrees

HardwareVisualizer development routinely runs several git worktrees at once
(review branches, parallel implementation lanes, Codex or Claude sessions). By
default Cargo gives every checkout its own `target/`, so each worktree's
intermediate build artifacts pile up separately and are easy to lose track of.
With the `duckdb-archive` feature enabled a single debug `target/` is roughly 5
to 7 GB, of which about 3 GB is the bundled DuckDB C++ build under
`target/debug/build/libduckdb-sys-*`. On 2026-09-12 four concurrent worktrees
plus stray directories from deleted worktrees filled the disk.

## What the repository configures

`.cargo/config.toml` sets Cargo's `build.build-dir` to one machine-wide parent
directory, with a per-workspace hash segment:

```toml
[build]
build-dir = "{cargo-cache-home}/build/shared/{workspace-path-hash}"
```

`{cargo-cache-home}` resolves to `$CARGO_HOME` (usually `~/.cargo`).
`{workspace-path-hash}` resolves to a hash of the workspace's manifest path, so
each worktree still gets its own subtree, for example
`~/.cargo/build/shared/68/cb97a342c873e1`. Cargo 1.91 or newer is required for
`build-dir`; the pinned toolchain in `rust-toolchain.toml` satisfies this.

The split is:

- **`build-dir` (one parent directory, one subtree per worktree)** holds
  intermediate artifacts: dependency `.rlib` files, build-script output
  (including the DuckDB build), incremental caches, fingerprints, and test
  executables.
- **`target/` (per checkout)** keeps only final artifacts: the application
  binary, examples, and Tauri bundles under `target/release/bundle`. Every path
  that CI, `tauri dev`, `tauri build`, and the perf workflows read stays where
  it was.

### `{workspace-path-hash}` is required, not cosmetic

An earlier version of this configuration pointed every worktree at the same
flat `build-dir` path with no hash segment, intending to let worktrees reuse
each other's compiled dependencies. A GitHub Copilot/Codex review of
[PR #2110](https://github.com/shm11C3/HardwareVisualizer/pull/2110) found that
Cargo does not fingerprint a workspace's own path-based member packages (this
repository's `core` and `src-tauri` crates) by absolute source path under
`build-dir` the way it does under `target/`. Reproduced locally with cargo
1.98.1: two throwaway workspaces with the identical package name and version
but different source, pointed at the same flat `build-dir`, correctly shared
an unrelated unchanged path dependency, but the second workspace's `cargo run`
finished instantly and **ran the first workspace's binary**, printing the
wrong value. The same collision reproduced with this repository's own
`hardviz-core` package across two real worktrees on different branches.

This is exactly the situation multiple worktrees on different feature
branches create, so a flat shared `build-dir` can silently serve one lane's
build or test results to another lane building the same package. The
`{workspace-path-hash}` segment gives each distinct workspace root its own
subtree, which is the only combination confirmed safe. Do not remove it.

### What benefit remains after the fix

Because each worktree's own package artifacts must stay isolated, and Cargo
keys the whole `build-dir` subtree (not just the workspace's own packages) by
`{workspace-path-hash}`, **worktrees with diverging source no longer share
compiled dependency crates either.** Measured on 2026-09-12: building
`hardviz-core` from two worktrees on different branches, each independently
compiled `sqlx` and its proc-macro crates and ended up with its own ~840 MB
subtree; nothing was deduplicated between them.

What this configuration still buys:

- A worktree's own `target/` stays tiny (final binaries only), so copying,
  backing up, or `git worktree remove`-ing a checkout is cheap regardless of
  how much it built.
- Every worktree's intermediate artifacts live under one parent directory
  (`~/.cargo/build/shared/`), so checking total build-cache disk usage or
  wiping it is one command instead of walking every worktree individually.
- Rebuilding the **same worktree path** after deleting its own subtree (for
  example after a `cargo clean` equivalent, or a machine restart) is a full
  cache hit if its dependency versions are unchanged, since the hash is a
  function of the workspace path, not its content. Two different worktree
  directories never share a subtree this way, even when they happen to be
  checked out to the same commit, because they are different paths.

It does **not** reduce total disk usage across worktrees whose source differs:
each worktree's build-dir subtree still ends up close to full size (roughly 5
to 7 GB with `duckdb-archive`), since Cargo still needs every dependency
artifact physically present in that subtree to link against. Disk usage is
bounded by the manual cleanup commands below, not automatically.

### Optional: speed up rebuilds with sccache

For the recompilation itself (not disk usage), Cargo's own build-cache
documentation recommends [sccache](https://github.com/mozilla/sccache), a
compiler-invocation cache that keys by preprocessed source content rather than
filesystem path, so it cannot repeat the correctness bug above. This is a
**per-machine developer convenience, not a repository setting**: enabling it
project-wide would break anyone's build the moment sccache is not on their
`PATH`, so configure it only in your own `~/.cargo/config.toml`, never in this
repository's. (CI enables it per job through workflow environment variables
instead; see [below](#ci-caches-the-duckdb-c-build-with-sccache).)

```toml
[build]
rustc-wrapper = "sccache"

[env]
SCCACHE_DIR = { value = "/absolute/path/to/a/cache/dir", force = false }
SCCACHE_CACHE_SIZE = { value = "20G", force = false }
```

Install with `brew install sccache` (or your platform's equivalent), then run
`sccache --stop-server` once after first setting this up so a stale server
process is not still holding an old configuration.

Measured on 2026-09-13 with cargo 1.98.1 and sccache 0.17.0, building
`hardviz-core` with `duckdb-archive` from two real worktrees on different
branches: a cold build took 5 m 57 s; the second worktree, with sccache warm,
took 5 m 03 s (about 15% faster). The improvement is real but modest, and
uneven by language: Rust compilations hit the cache well (56%), but the
DuckDB C/C++ build hit poorly (9%). This is a direct consequence of
`{workspace-path-hash}`: it makes each worktree extract DuckDB's bundled
source into its own absolute path, and that path is embedded in preprocessor
output, which changes sccache's C/C++ cache key even though the underlying
source is identical. sccache also adds its own cache directory on disk
(capped by `SCCACHE_CACHE_SIZE` above); it does not shrink any worktree's own
build-dir subtree.

## What changes for you

- The first build after upgrading repopulates the shared parent directory;
  existing per-worktree `target/` directories are no longer read for
  intermediate artifacts and can be deleted.
- Two builds of the **same profile in the same worktree** running at the same
  time still serialize on Cargo's build-directory lock (`Blocking waiting for
  file lock on build directory`), same as stock Cargo does for one `target/`
  today. Builds in different worktrees no longer contend with each other at
  all, since each has its own subtree.
- `cargo clean` from one worktree only removes that worktree's own subtree
  under the shared parent, not other worktrees' artifacts.
- To opt out for one command, set the environment variable
  `CARGO_BUILD_BUILD_DIR` (for example to `target`), which overrides the
  config file.

## CI keeps the build dir under `target/`

`.github/actions/setup-rust/action.yml` exports
`CARGO_BUILD_BUILD_DIR={workspace-root}/target` before `Swatinem/rust-cache`
runs, so on GitHub-hosted runners the stock layout applies and the repository
setting above is local-only. rust-cache saves and prunes only the workspace
`target/` directories it is told about; it does not read `build.build-dir`.
After this setting landed, CI run 34850128330 (`test-core`, windows-latest)
restored a 697 MB `develop` entry whose "Cache Paths" were `$CARGO_HOME`
registry/git/bin plus `target/`, then compiled all 249 crates into
`C:\Users\runneradmin\.cargo\build\shared\...\debug\deps` in 13 m 53 s: the
entry held the registry but none of the compiled crates. Listing the hashed
build dir as a rust-cache `cache-directories` entry was rejected because such
directories are saved wholesale (workspace crates and test binaries included)
and never pruned, and because `{workspace-path-hash}` ties the path to the
runner's checkout location.

The same action takes a required `cache-key` input, one per job kind
(`core-test`, `tauri-lint`, ...). Only `develop` saves, and the first job to
finish with a given key is the only one that saves it, so a key shared by a
`clippy` job and a `test` job stores whichever finished first, which the other
cannot reuse. Per-kind keys cost one entry per kind, platform, and lockfile
hash against GitHub's 10 GB per-repository cache limit; if `gh cache list`
shows eviction thrash, drop `cache-targets` for low-value kinds before
allowing pull requests to save.

`publish.yml` only runs on tag pushes, where save-if is always false, so its
rust-cache step reuses `tauri-build` (populated by `ci.yml`'s `test-build` job
on `develop`) instead of a job-specific key nothing would ever save to.

## CI caches the DuckDB C++ build with sccache

`.github/actions/cache-duckdb/action.yml` installs sccache
(`mozilla-actions/sccache-action`) and exports `RUSTC_WRAPPER=sccache`. The
`ci.yml` jobs that compile with `--features duckdb-archive` (`lint-core`,
`test-core`, `lint-tauri`) run it right after `setup-rust`. The `cc` crate
treats `RUSTC_WRAPPER=sccache` as a C/C++ compiler wrapper, so every `cl.exe`
/ `c++` invocation of the bundled DuckDB build (326 translation units, about
9 minutes of the 13 to 14 minute cold build in run 34850128330) is keyed on
preprocessed source, flags and compiler. Non-incremental Rust dependency
crates are cached the same way; workspace crates are passed through. sccache
prints hit/miss statistics in its post step, which is the evidence surface
for whether the cache is working.

A dedicated `actions/cache` entry holding
`target/debug/build/libduckdb-sys-*/out` was rejected: Cargo has no early
cutoff, so a `run-build-script` unit is re-run whenever its
`build-script-build` binary or any of that binary's dependencies (`cc`,
`bindgen`, `syn`, `ureq`, ...) was rebuilt in the same invocation, and also
whenever the binary's mtime is newer than the restored `output` file. The
restored directory would survive only when the whole build-dependency closure
is already fresh in `target/`, which is the case rust-cache already covers.

A first version of this action set `SCCACHE_GHA_ENABLED=true`, routing every
cache write through GitHub's Actions Cache Service as an individual real-time
PUT issued from inside the compile request. That service rate-limits writes
per *workflow run*, shared across every job in it, and sccache's GHA backend
installs no retry layer, so a rate-limited write is dropped and counted as a
write error rather than retried
([mozilla/sccache#2821](https://github.com/mozilla/sccache/issues/2821), open
and unfixed; a 2023 attempt to add the missing retry layer,
[mozilla/sccache#1700](https://github.com/mozilla/sccache/pull/1700), was
closed unmerged for accumulated conflicts). This repository runs
`duckdb-archive` in three job kinds across three platforms, up to nine
concurrent writers in one workflow run. Verification on run 34865702849
confirmed the predicted failure mode in every one of those jobs: 0 cache hits
and effectively 100% write errors (for example `test-core (windows-latest)`:
325 compile requests, 0 hits, 239 write errors), and that job finished slower
than the pre-sccache baseline.

The action now uses sccache's default local disk cache
([docs/Local.md](https://github.com/mozilla/sccache/blob/main/docs/Local.md))
instead: `SCCACHE_DIR` points at a fixed directory under `runner.temp`, and a
plain `actions/cache` step persists that one directory, the same mechanism
`setup-rust`'s rust-cache step and this repository's AppRun/WiX caches already
use. Persistence becomes one archive upload per job at job end instead of
hundreds of live per-object writes, so it is not subject to the per-write
rate limit.

Known limits: a Cargo.lock change inside libduckdb-sys's build-dependency
closure changes the unit's metadata hash and therefore the absolute `OUT_DIR`
that ends up in the preprocessed output, so the first run after such a bump
recompiles DuckDB once. The wrapping `actions/cache` key includes the
workspace Cargo.lock hash, so each distinct lockfile state gets its own
archive, while its `restore-keys` prefix still restores the nearest older
archive so unrelated Cargo.lock churn does not force a cold sccache
directory. Saves are not restricted to `develop`: because the key changes
only when Cargo.lock changes rather than on every run, a branch whose
lockfile matches `develop` reaches the same key and skips its own save, so
pull requests saving does not multiply entries per run; `SCCACHE_CACHE_SIZE`
is capped at 1G per job to bound growth, watch `gh cache list` if it still
grows faster than expected. The action runs after rust-cache on purpose:
rust-cache folds `RUST*` environment variables into its key, so exporting
`RUSTC_WRAPPER` earlier would split the rust-cache key between jobs with and
without sccache.

## Keeping disk usage bounded

The shared parent directory still accumulates one subtree per worktree you
have ever built, including worktrees you later deleted. Reclaim all of it
occasionally with:

```bash
rm -rf ~/.cargo/build/shared
```

There is currently no automatic pruning tied to worktree removal; treat this
as a manual step alongside worktree cleanup below.

Do not run `rm -rf target` in the main checkout without looking first: some
tools (Codex, for example) create their worktrees under
`target/codex-worktrees/`, and deleting `target/` there deletes those
checkouts, including uncommitted work. Delete `target/debug` and
`target/release` instead, or move the worktrees out first.

Worktree hygiene matters as much as the cache. `git worktree remove` deletes
the checkout, but its shared-cache subtree under `~/.cargo/build/shared/` is
keyed by the now-gone path and is not cleaned up automatically; it sits there
until the periodic `rm -rf ~/.cargo/build/shared` above reclaims it. Drop
worktree registrations whose directories were deleted by hand with:

```bash
git worktree prune
```

To list every worktree together with what its checkout still occupies:

```bash
git worktree list --porcelain | while IFS= read -r line; do
  case "$line" in
    "worktree "*) du -sh "${line#worktree }" ;;
  esac
done
```
