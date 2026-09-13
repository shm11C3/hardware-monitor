import { readFileSync } from "node:fs";

const lockfile = readFileSync("Cargo.lock", "utf8");
const notice = readFileSync(
  "docs/licenses/manual/duckdb-bundled-libraries.md",
  "utf8",
);

const lockVersion = lockfile.match(
  /name = "libduckdb-sys"\nversion = "([^"]+)"/,
)?.[1];
const coveredVersion = notice.match(
  /^Covered libduckdb-sys version: `([^`]+)`$/m,
)?.[1];

if (!lockVersion) {
  throw new Error("Cargo.lock does not contain libduckdb-sys");
}

if (!coveredVersion) {
  throw new Error(
    "DuckDB bundled-library notice does not declare its covered libduckdb-sys version",
  );
}

if (lockVersion !== coveredVersion) {
  throw new Error(
    `DuckDB bundled-library notice covers libduckdb-sys ${coveredVersion}, but Cargo.lock pins ${lockVersion}. Review and update the manual notice before changing the pinned version.`,
  );
}

console.log(`DuckDB license notice covers libduckdb-sys ${lockVersion}`);
