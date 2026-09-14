/**
 * Fail when the bundled DuckDB changes without the hand-maintained attribution
 * for its C/C++ libraries being refreshed.
 *
 * Those libraries ship no LICENSE/NOTICE files inside `duckdb.tar.gz`, so no
 * Rust-side tool can see them and `docs/licenses/manual/` is the only place
 * they are attributed (#2111). Two independent inputs decide which of them end
 * up in the binary, so both are compared against what the entry records:
 *
 * - the pinned crate versions in Cargo.lock, since a version bump can add,
 *   drop or relicense a vendored library;
 * - the `duckdb` dependency's feature configuration in core/Cargo.toml, since
 *   enabling an extension such as `json` or `parquet` compiles a different set
 *   of vendored libraries at unchanged versions.
 */
import { readFileSync } from "node:fs";

const lockPath = process.argv[2] ?? "Cargo.lock";
const noticePath =
  process.argv[3] ?? "docs/licenses/manual/duckdb-bundled-c-cpp.md";
const manifestPath = process.argv[4] ?? "core/Cargo.toml";

const CRATES = ["libduckdb-sys", "duckdb"] as const;

/** Version of `crate` as pinned in the Cargo.lock text, or null when absent. */
const lockedVersion = (lock: string, crate: string): string | null => {
  const match = lock.match(
    new RegExp(`\\[\\[package\\]\\]\\nname = "${crate}"\\nversion = "([^"]+)"`),
  );
  return match?.[1] ?? null;
};

/** Value recorded by a `Covered <subject>:` line of the notice. */
const coveredValue = (notice: string, subject: string): string | null => {
  const match = notice.match(new RegExp(`^Covered ${subject}: *(.+?) *$`, "m"));
  return match?.[1] ?? null;
};

/**
 * The `duckdb` dependency's feature configuration from a Cargo manifest,
 * normalised to `default-features = <bool>, features = ["a", "b"]` with the
 * feature names sorted, so that reordering the list alone is not a failure.
 * Returns null when the manifest declares no `duckdb` dependency.
 */
const declaredFeatures = (manifest: string): string | null => {
  const dependency = manifest.match(/^duckdb\s*=\s*\{([\s\S]*?)\}/m)?.[1];
  if (dependency === undefined) return null;

  const defaultFeatures =
    dependency.match(/default-features\s*=\s*(true|false)/)?.[1] ?? "true";
  const featureList = dependency.match(/features\s*=\s*\[([\s\S]*?)\]/)?.[1];
  const features = [...(featureList ?? "").matchAll(/"([^"]+)"/g)]
    .map((match) => match[1])
    .sort();

  return `default-features = ${defaultFeatures}, features = [${features
    .map((feature) => `"${feature}"`)
    .join(", ")}]`;
};

const lock = readFileSync(lockPath, "utf8");
const notice = readFileSync(noticePath, "utf8");
const manifest = readFileSync(manifestPath, "utf8");

const problems: string[] = [];

for (const crate of CRATES) {
  const locked = lockedVersion(lock, crate);
  const covered = coveredValue(notice, `${crate} version`);

  if (covered === null) {
    problems.push(
      `${noticePath} has no "Covered ${crate} version:" line. Add one recording the version the attribution was read against.`,
    );
    continue;
  }
  if (locked === null) {
    problems.push(
      `${crate} is not in ${lockPath}, but ${noticePath} still claims to cover ${covered}. Remove the entry if the bundled DuckDB no longer ships.`,
    );
    continue;
  }
  if (locked !== covered) {
    problems.push(
      `${crate} is pinned to ${locked} in ${lockPath} but ${noticePath} covers ${covered}. Re-read the bundled third-party license headers for ${locked} (see "How to refresh this entry") and update the covered version.`,
    );
  }
}

const declared = declaredFeatures(manifest);
const coveredFeatures = coveredValue(notice, "duckdb features");

if (coveredFeatures === null) {
  problems.push(
    `${noticePath} has no "Covered duckdb features:" line. Add one recording the duckdb dependency's feature configuration the attribution was read against.`,
  );
} else if (declared === null) {
  problems.push(
    `${manifestPath} declares no duckdb dependency, but ${noticePath} still claims to cover ${coveredFeatures}. Remove the entry if the bundled DuckDB no longer ships.`,
  );
} else if (declared !== coveredFeatures) {
  problems.push(
    `${manifestPath} builds duckdb with ${declared} but ${noticePath} covers ${coveredFeatures}. A different feature set compiles a different set of vendored C/C++ libraries, so re-derive the compiled set (see "How to refresh this entry") and update the covered features.`,
  );
}

if (problems.length > 0) {
  for (const problem of problems) {
    console.error(`❌ ${problem}`);
  }
  process.exit(1);
}

console.log(
  `✅ Bundled DuckDB attribution covers the pinned crate versions (${CRATES.map(
    (crate) => `${crate} ${lockedVersion(lock, crate)}`,
  ).join(", ")}) and the declared feature set (${declared}).`,
);
