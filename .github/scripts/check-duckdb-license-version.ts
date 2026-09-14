import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

const lockfile = readFileSync("Cargo.lock", "utf8");
const notice = readFileSync(
  "docs/licenses/manual/duckdb-bundled-libraries.md",
  "utf8",
);

/** Version of `crate` as pinned in Cargo.lock. */
const pinnedVersion = (crate: string) =>
  lockfile.match(
    new RegExp(`name = "${crate}"\\r?\\nversion = "([^"]+)"`),
  )?.[1];

/**
 * The features Cargo actually resolves for the `duckdb` package in a
 * duckdb-archive build, sorted. Which vendored C/C++ libraries compile into
 * the engine depends on these, not only on the pinned versions: enabling an
 * extension such as `json` or `parquet` changes the compiled set at unchanged
 * versions.
 *
 * Read from the resolved graph rather than from the dependency declaration in
 * `core/Cargo.toml`, because a feature can also be turned on by forwarding
 * from the `[features]` table (`duckdb-archive = ["dep:duckdb",
 * "duckdb/json"]`), which leaves that declaration untouched. `--locked` keeps
 * the check from silently resolving against an updated lockfile; a failure
 * here propagates and fails the guard.
 */
const resolvedFeatures = () => {
  const metadata = JSON.parse(
    execSync(
      "cargo metadata --format-version 1 --features duckdb-archive --locked",
      { encoding: "utf8", maxBuffer: 100 * 1024 * 1024 },
    ),
  );

  const version = pinnedVersion("duckdb");
  const duckdb = metadata.packages.find(
    (pkg: { name: string; version: string }) =>
      pkg.name === "duckdb" && pkg.version === version,
  );
  if (!duckdb) {
    throw new Error(
      `cargo metadata reports no duckdb ${version} package for a duckdb-archive build`,
    );
  }

  const node = metadata.resolve.nodes.find(
    (candidate: { id: string }) => candidate.id === duckdb.id,
  );
  if (!node) {
    throw new Error(`cargo metadata resolved no node for ${duckdb.id}`);
  }

  return [...node.features].sort().join(", ");
};

const covered = (subject: string) =>
  notice.match(new RegExp(`^Covered ${subject}: \`([^\`]+)\``, "m"))?.[1];

for (const crate of ["libduckdb-sys", "duckdb"]) {
  const lockVersion = pinnedVersion(crate);
  const coveredVersion = covered(
    crate === "duckdb" ? "`duckdb` version" : `${crate} version`,
  );

  if (!lockVersion) {
    throw new Error(`Cargo.lock does not contain ${crate}`);
  }

  if (!coveredVersion) {
    throw new Error(
      `DuckDB bundled-library notice does not declare its covered ${crate} version`,
    );
  }

  if (lockVersion !== coveredVersion) {
    throw new Error(
      `DuckDB bundled-library notice covers ${crate} ${coveredVersion}, but Cargo.lock pins ${lockVersion}. Review and update the manual notice before changing the pinned version.`,
    );
  }
}

const features = resolvedFeatures();
const coveredFeatures = covered("duckdb features");

if (!coveredFeatures) {
  throw new Error(
    "DuckDB bundled-library notice does not declare its covered duckdb features",
  );
}

if (features !== coveredFeatures) {
  throw new Error(
    `DuckDB bundled-library notice covers duckdb features ${coveredFeatures}, but a duckdb-archive build resolves ${features}. A different feature set compiles a different set of vendored C/C++ libraries, so re-review the archive before changing the features.`,
  );
}

console.log(
  `DuckDB license notice covers libduckdb-sys ${pinnedVersion("libduckdb-sys")}, duckdb ${pinnedVersion("duckdb")} with resolved features [${features}]`,
);
