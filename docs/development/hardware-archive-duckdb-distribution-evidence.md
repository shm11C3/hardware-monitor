# Native DuckDB Distribution And Durability Evidence

Status: evidence and plans for adoption blockers still listed in
[#2085](https://github.com/shm11C3/HardwareVisualizer/issues/2085) and
[#2084](https://github.com/shm11C3/HardwareVisualizer/issues/2084), under
[#2052](https://github.com/shm11C3/HardwareVisualizer/issues/2052). The
`duckdb-archive` feature is not enabled in any production build, no runtime
behavior changed, and no product code was added. Commands, exit codes,
environment, per-run load averages and the full third-party inventory are in the
[artifact](benchmarks/hardware-archive-duckdb-distribution-2026-09-12.json).
Measured on macOS 26.6.2 arm64 (Apple M4, 10 cores, 24 GiB) at
`1839986f6e29422420d5fcb4d49337e7bed1e31e`, rustc 1.98.1, Apple clang 17.0.0,
`duckdb 1.10505.0` with bundled DuckDB 1.5.5.

**Implication.** The remaining distribution blockers are narrower than #2085
implies: the feature already compiles and runs in CI on Windows x64, Linux x64
and macOS arm64, so what is left is macOS x64 (no feature-enabled job; its
release build has the feature disabled), the absence of
any job that links an application binary or installer with the feature. The
license gate and notice generator, which both ran with default features and so
never saw the DuckDB tree, were fixed in #2123 (see "License and packaging").
Durability is unchanged — the engine issues a real
per-commit flush on all three platforms, but only app-crash evidence exists.

## Measured

Both builds used `--release --locked --offline` with a fresh `CARGO_TARGET_DIR`
and exited 0. `Cargo.lock` was not modified.

```bash
cargo build --release --locked --offline -p hardviz-core --lib
cargo build --release --locked --offline -p hardviz-core --lib --features duckdb-archive
```

| Measurement | Without | With feature | Delta |
| --- | ---: | ---: | ---: |
| `hardviz-core` rlib bytes | 18,349,744 | 23,281,552 | +4,931,808 |
| rlib gzip -9 bytes | 4,507,872 | 5,715,878 | +1,208,006 |
| `target/` KiB | 548,796 | 1,038,660 | +489,864 |
| `libduckdb-sys` static archive bytes | 0 | 70,018,256 | +70,018,256 |
| Clean build s | 159.35 | 233.30 | +73.95 |

The rlib is not shipped and the compiled DuckDB C++ sits in the separate static
archive, before linking, LTO or stripping, so **no row here is the installed
size delta**. **The timings are not usable**: three other agent lanes compiled
this workspace concurrently (load 6.9–102 on 10 cores), and the with-feature
build reports a *faster* no-op rebuild (0.53 s vs 1.60 s) and touched-source
rebuild (9.34 s vs 37.00 s) than the strictly smaller baseline — physically
implausible, and itself the evidence that contention dominated.

Dependency growth: 432 → 473 crates, **41 added** (at the measured commit;
`-e normal,build` omits proc-macro edges, so the real graph is wider — see the
licence section). `ureq`, `zip`, `zopfli` and `zlib-rs` enter only as *build*
dependencies of `libduckdb-sys`; the `arrow-*` crates link in. Because
`core/Cargo.toml` uses `default-features = false, features = ["bundled"]`, the
`cc` backend compiles the manifest's `base` section together with the
always-enabled `core_functions` extension, and neither `json` nor `parquet`:
280 translation units, 12 vendored C/C++ libraries with sources of their own
(see the licence section for the 9 header-only ones this count missed).

**Not measured.** `cargo build --release -p hardware_visualizer --features
custom-protocol,duckdb-archive` failed with `No space left on device (os error
28)` after the shared volume fell from 24 GiB to 132 MiB under four concurrent
lanes. It never reached a link step — an environment failure, not a product one
— and was not retried because a contended retry would produce another unusable
timing. The baseline half completed: 13,487,440 bytes, gzip 6,306,422, `target/`
2.25 GiB. No Tauri bundle was produced, so no installer size delta exists, and
Windows x64, Linux x64 and macOS x64 are unmeasured on every axis.

## License and packaging

`libduckdb-sys` and `duckdb` are MIT (Stichting DuckDB Foundation).

### Tooling state before #2123 (measured 2026-09-12)

Everything in this subsection is the pre-fix measurement. It is kept because
the exit codes are the evidence the fix was needed; it does not describe the
repository today. For the current state see "Tooling state after #2123" below.

The license gate did not see the DuckDB crates: `deny.toml` set `[graph]
all-features = false`, and with default features `cargo metadata` returned no
`duckdb` node at all. `.github/scripts/generate-licenses.ts` shared the blind
spot, invoking `cargo license` and `cargo metadata` without features.

Running the gate both ways locally with cargo-deny 0.19.4 (the version CI
installs) showed this was not merely a coverage gap. Without the feature it
exited 0 (`licenses ok`, matching CI at the time); **with `--features
duckdb-archive` it exited 4** — `tiny-keccak 2.0.2` is `CC0-1.0`, which was not
on the allow list, reached via `ahash → const-random → const-random-macro`
under `arrow-array → arrow → duckdb`. An earlier revision of this document
predicted the check "would pass if it ran"; that prediction was wrong. It came
from the 41 crates `cargo tree -e normal,build` enumerates, a listing that omits
proc-macro edges and so never shows `tiny-keccak`, while cargo-deny walks the
full graph. Enabling the feature in a build the license job evaluated would
therefore have **failed** it until `CC0-1.0` was allowed or that path avoided.

### Packaging

Notices do reach users: `publish.yml` regenerates `tmp/THIRD_PARTY_NOTICES.md`
just before `tauri-action`, and `tauri.conf.json` bundles it as a resource
beside `LICENSE`; the same file is also a release asset. The committed copy is a
29-byte placeholder.

### Vendored C/C++ attribution

Crate metadata cannot describe the vendored C++, and no tooling fix changes
that. The `duckdb.tar.gz` ships **no** `LICENSE`, `COPYING` or `NOTICE` file,
yet 21 libraries compile into the binary — MIT, Apache-2.0, BSD-3-Clause,
BSD-2-Clause, Zlib, BSL-1.0, the Unlicense, the PostgreSQL License, a Bison
skeleton under GPL-2.0-or-later *with* its special exception, and three
dual-licensed libraries (`mbedtls`, `zstd`, `pcg`). Each offers a
GPL-3.0-compatible arm, so nothing conflicts with the application's
GPL-3.0-or-later licence, but all carry attribution obligations. This is a
reading of source headers, not a legal review.

An earlier revision of this section counted 12. That was the set with `.cpp`
files of their own in `duckdb/manifest.json`; it missed 9 header-only libraries
(`concurrentqueue`, `fast_float`, `httplib`, `jaro_winkler`, `pcg`, `pdqsort`,
`ska_sort`, `tdigest`, `vergesort`) that reach the binary because compiled
DuckDB sources include their headers — for example
`src/parallel/task_scheduler.cpp` includes `concurrentqueue.h`, and the
always-enabled `core_functions` source `approximate_quantile.cpp` includes
`t_digest.hpp`. `brotli`, `lz4`, `snappy` and `thrift` remain out of scope:
they belong to the `parquet`/`json` manifest sections, which this feature set
does not build.

### Tooling state after #2123

This is the current state; it supersedes "Tooling state before #2123" above.
`deny.toml` carries a crate-scoped `CC0-1.0` exception for `tiny-keccak`, and
`license-check-cargo` runs cargo-deny with `--features duckdb-archive`, so the
gate evaluates the graph a `duckdb-archive` build ships.
`generate-licenses.ts` takes the feature as an argument and passes it to
`cargo license` and `cargo metadata`, so the feature-only crates reach the
generated notices. `docs/licenses/manual/duckdb-bundled-libraries.md` carries
the attribution for the bundled C/C++ libraries — all 21 of them after this
change; #2123 covered the 12 with sources of their own — and
`.github/scripts/check-duckdb-license-version.ts` fails CI when the pinned
`duckdb` / `libduckdb-sys` versions in `Cargo.lock`, or the `duckdb`
dependency's feature configuration in `core/Cargo.toml`, move past what that
notice records.

### Extension autoload defaults

Unrelated to the license work: the bundled build compiles
with `DUCKDB_EXTENSION_AUTOINSTALL_DEFAULT=1` and `..._AUTOLOAD_DEFAULT=1`,
countered only at open time by `enable_autoload_extension(false)` and `SET
enable_external_access = false` — worth an explicit decision against the
no-outbound-telemetry principle.

## Supported-target matrix

Feature-enabled coverage only. Every released installer is a feature-*disabled*
build, on every target.

| Target | Compiled | Executed | Binary linked | Installer |
| --- | --- | --- | --- | --- |
| Windows x64 | yes (`lint-core`, `lint-tauri`) | yes (`test-core`) | no | no |
| Linux x64 | yes | yes | no | no |
| macOS arm64 | yes | yes | no | no |
| macOS x64 | **no** | no | no | no |

`test-core` runs `cargo test -p hardviz-core --features duckdb-archive` on all
three runner platforms. macOS x64 appears in no CI matrix at all; the
`publish-tauri` matrix does cross-compile `x86_64-apple-darwin`, but `publish.yml`
never mentions `duckdb-archive`, so that is feature-disabled release coverage
and not evidence the feature builds there. CI never bundles an installer
(`--no-bundle`).

`Swatinem/rust-cache` used, at the time of this evidence, `shared-key:
"rust-workspace"`, `add-job-id-key: false` and `save-if` restricted to
`develop`. (The action has since moved to one `cache-key` per job kind and
pins the Cargo build dir under `target/` in CI; see
[`local-build-cache.md`](local-build-cache.md#ci-keeps-the-build-dir-under-target).
The analysis below describes the configuration this evidence was gathered
under.) Reading `src/config.ts` at the
pinned SHA `6323deb1`, `add-job-id-key: false` removes *only* the job id: the
key always ends with `-${runnerOS}-${runnerArch}` ("to avoid cross-contamination
of cache", per the source comment), and `add-rust-environment-hash-key` — on by
default and not overridden here — further folds in the rust version, env and
lockfile hashes. Windows, Linux and macOS therefore never share an entry. What
the shared key does is make all Rust jobs on the *same* platform, toolchain and
lockfile share one, and no pull request saves; within such a group a `develop`
job *without* the feature can save an entry lacking the DuckDB objects that a
later same-platform job restores, so the compile cost is not reliably amortised.

Smallest change closing the linked-binary gap. **Not applied.**

```diff
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
       - name: Build Tauri Rust crate (CI optimized for PR)
-        run: cargo build -p hardware_visualizer --profile ci --features custom-protocol
+        run: cargo build -p hardware_visualizer --profile ci --features custom-protocol,duckdb-archive
```

Estimated cost: the local cold delta was ~74 s on 10 M4 cores under load, and
GitHub runners have 4 slower cores, so budget roughly 5–12 minutes of added cold
wall time, Windows MSVC at the upper end. The three platforms run in parallel,
so that is added critical-path time, not three times the cost. This is
arithmetic over a contended local measurement, not an observed CI run. macOS x64
needs a separate matrix entry with `targets: x86_64-apple-darwin`; the installer
gap needs a bundling job that does not exist.

## Durability plan for #2084

**Engine behavior.** Every commit runs
`SingleFileStorageCommitState::FlushCommit()` → `WriteAheadLog::Flush()`
(`src/storage/write_ahead_log.cpp:542`), appending a `WAL_FLUSH` marker then
calling `writer->Sync()`. `LocalFileSystem::FileSync` uses
`fcntl(fd, F_FULLFSYNC)` on macOS with an fsync fallback, `fdatasync`/`fsync` on
Linux, `FlushFileBuffers` on Windows. Recovery (`wal_replay.cpp`) commits only
at `WAL_FLUSH` markers, checksums each entry, and — `abort_on_wal_failure` being
off by default — silently truncates a torn tail while rethrowing genuine
corruption. Defaults: `checkpoint_threshold` 16 MiB,
`wal_autocheckpoint_entries` disabled, `checkpoint_on_shutdown` true.
`native_config()` sets only `ReadWrite`, `threads(2)`, `max_memory("128MB")`,
`enable_autoload_extension(false)`, an owned spill directory and
`enable_external_access = false` — **no** checkpoint or WAL option, so those
defaults apply unreviewed.

**App-crash evidence is not power-loss evidence.** The 2026-09-06/07 SIGKILL
probes leave the page cache and kernel intact; they exercise WAL replay, not
stable storage. Proposed validation, in increasing cost:

1. On this macOS host, run the existing lifecycle probe under `sudo fs_usage -w
   -f filesys` and confirm one `F_FULLFSYNC` against the `.wal` per committed
   batch. Proves only that the durability call is issued; proves nothing about
   what survives, on any platform.
2. On Linux, place the database on a `dm-log-writes` target, then replay the
   recorded log onto a scratch device up to each cut point and open the result,
   asserting that every commit acknowledged before the cut survives, no batch is
   partial, no checksum fails, and the identity high-water never regresses.
   Preferred over `dm-flakey` because cut points are enumerated deterministically
   at flush boundaries rather than dropped at random. Proves Linux crash
   consistency against a device retaining only pre-cut flushed writes. Cannot
   prove NTFS or APFS behavior, drive firmware honesty, or a failure rate.
3. For Windows, run the guest on a Linux host with its virtual disk backed by a
   host `dm-log-writes` device and `cache=none`, then replay to each cut point
   and open the archive; a copy-on-write snapshot backend that can discard every
   post-cut write is an acceptable substitute. Proves the `FlushFileBuffers` and
   NTFS path is crash-consistent under a device that drops post-cut writes.

A QEMU guest with `cache=writeback` killed host-side is **not** a valid
power-loss test: those writes were already accepted into the *host* page cache,
so killing the process loses nothing and the host writes them back anyway. A
physical power cut remains the only proof that a drive honours the flush, and is
out of scope. macOS has no `dm-log-writes` equivalent, so beyond tier 1 there is
**no in-scope mechanism** for APFS crash consistency — that stays an open gap,
not inferred evidence. None of this covers disk exhaustion during checkpoint or
compaction, corruption outside the WAL, or a second concurrent instance.

## File format compatibility

`VERSION_NUMBER` is 64 with a readable range of `[64, 68]`.
`SerializationCompatibility::Default()` returns `v0.10.2` (serialization version
1) unless `DUCKDB_LATEST_STORAGE` or `DUCKDB_ALTERNATIVE_VERIFY` is defined, and
`build_bundled_cc.rs` defines neither, so `GetVersionNumber()` returns 64. Files
this application creates carry header version 64, the oldest format this engine
emits.

The declared older-reader floor is therefore **v0.10.2** — the version
`SerializationCompatibility::Default()` literally names. Header 64 is shared by
the release names v0.9.0 through v1.1.3 in `storage_version_info`, but a shared
header number is not proof that a v0.9.0 binary can read what this 1.5.5 build
writes, and nothing in the source guarantees it. **Any claim about readers older
than v0.10.2 is untested**: this lane produced no with-feature binary, and
obtaining an older reader would need a network download or a new dependency.

The crate version does **not** determine the file format, and pinning
`=1.10505.0` does not pin it. The hazard is the reverse direction:
`single_file_block_manager.cpp:1321` silently rewrites the header from 64 to 65
once the storage version reaches 4, so anything raising it — an explicit
`STORAGE_VERSION`, a future crate default, or a feature requiring it —
permanently makes the archive unreadable by any reader whose accepted range ends
below 65. That is narrower than "older builds": a build accepts
`[VERSION_NUMBER_LOWER, VERSION_NUMBER_UPPER]`, and `storage_version_info` maps
65 to the v1.2.x names, so releases from that era onward are unaffected. The
policy should
assert the written storage version explicitly instead of inheriting the default,
record it beside `schema_version` in the native metadata table, and treat a
crate bump as a format review rather than a routine dependency update.

## Limits

Single run per configuration, macOS arm64 only, all timings unusable (heavy
concurrent load; two of three deltas physically implausible). Unmeasured: the
with-feature application binary, every installer, and all other targets. The CI
estimate is arithmetic, not an observed run. Licences were read from source
headers in a tarball that ships no licence files — not a legal review. The
durability work is a plan; no OS-crash or power-loss test ran. Storage-version
findings come from source constants, since no with-feature binary existed to
create and inspect a DuckDB file with.
