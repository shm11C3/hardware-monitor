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
- Two worktrees that happen to be **on the same commit** (for example, a
  review checkout of the exact branch under test) do share their subtree,
  since the hash is based on path, not content, and identical path-based
  packages with identical content fingerprint identically either way.

It does **not** reduce total disk usage across worktrees whose source differs,
and it does not avoid recompiling shared third-party dependencies (DuckDB,
sqlx, tokio, and the rest) on each diverging worktree. If that is what you
need, use a content-addressed compiler cache instead: Cargo's own build-cache
documentation recommends [sccache](https://github.com/mozilla/sccache) for
sharing compiled dependencies across separate workspace checkouts, since it
keys its cache by compiler invocation and preprocessed source content rather
than by filesystem path, so it cannot repeat this bug. `sccache` is not
configured in this repository as of this writing.

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
the checkout but leaves nothing behind, since the checkout no longer owns a
large `target/`. Drop registrations whose directories were deleted by hand
with:

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
