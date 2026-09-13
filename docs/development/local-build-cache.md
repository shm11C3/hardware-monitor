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
project-wide would break anyone's (and CI's) build the moment sccache is not
on their `PATH`, so configure it only in your own `~/.cargo/config.toml`, never
in this repository's:

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
