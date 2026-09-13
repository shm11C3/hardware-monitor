import { execSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

type NpmLicenseInfo = {
  licenses: string;
  repository?: string;
  publisher?: string;
  email?: string;
  licenseFile?: string;
};

type CargoLicenseInfo = {
  name: string;
  version: string;
  license: string;
  repository?: string;
  description?: string;
};

type CargoMetadata = {
  packages: CargoPackage[];
  resolve: {
    nodes: CargoResolveNode[];
  };
  workspace_members: string[];
};

type CargoPackage = {
  id: string;
  name: string;
  version: string;
  manifest_path: string;
};

type CargoResolveNode = {
  id: string;
  deps: CargoResolveDep[];
};

type CargoResolveDep = {
  pkg: string;
  dep_kinds: { kind: string | null }[];
};

// ==========================
// Argument processing
// ==========================
const target = process.argv[2]; // "linux" or "windows"
if (!target || !["linux", "windows", "macos", "tmp"].includes(target)) {
  console.error(
    "Usage: node --experimental-strip-types script.ts <linux|windows|macos|tmp>",
  );
  process.exit(1);
}

// Switch output directory based on OS
const outputDir =
  target === "tmp"
    ? path.resolve("./tmp")
    : path.resolve(`./docs/licenses/${target}`);
const outputPath = path.join(outputDir, "THIRD_PARTY_NOTICES.md");

const generateLicenseTxt = (
  name: string,
  licenses: string,
  repository?: string,
  publisher?: string,
  email?: string,
) => {
  let output = `## ${name}\n\n`;
  output += `- License: ${licenses}\n`;
  if (repository) output += `- Repository: [${repository}](${repository})\n`;
  if (publisher) output += `- Publisher: ${publisher}\n`;
  if (email) output += `- Email: <${email}>\n`;
  output += "\n";

  return output;
};

let output = "# THIRD_PARTY_NOTICES\n\n";

output +=
  "This application includes third-party libraries licensed under their respective licenses.\n\n";

//
// ====================
// 1. Node.js dependencies (prod only)
// ====================
//
try {
  // --excludePrivatePackages drops the app's own package.json entry (marked
  // "private": true so it never publishes to npm). Without it, license-checker
  // ignores our actual "license" field and force-labels the app UNLICENSED
  // (see license-checker/lib/index.js, `if (json.private) ... = UNLICENSED`).
  // This file is for third-party notices, so the app itself doesn't belong in it.
  const npmRawJson = execSync(
    "npx license-checker --production --excludePrivatePackages --json",
    {
      encoding: "utf8",
    },
  );
  const npmData = JSON.parse(npmRawJson);

  for (const [name, info] of Object.entries(npmData) as [
    string,
    NpmLicenseInfo,
  ][]) {
    output += generateLicenseTxt(
      name,
      info.licenses,
      info.repository,
      info.publisher,
      info.email,
    );

    // Get the contents of the license file
    if (info.licenseFile) {
      const licenseContent = readFileSync(info.licenseFile, {
        encoding: "utf8",
      });
      output += "```LICENSE\n";
      output += `${licenseContent.trim().replace(/```/g, "`` ``` ``")}\n`;
      output += "```\n\n";
    }
  }
} catch (e) {
  console.error("❌ Failed to collect NPM licenses:", e);
}

//
// ====================
// 2. Rust dependencies (filtered by metadata)
// ====================
//
try {
  const cargoJson = execSync("cargo license --features duckdb-archive --json", {
    encoding: "utf8",
  });
  const cargoData: CargoLicenseInfo[] = JSON.parse(cargoJson);

  const metadataJson = execSync(
    "cargo metadata --features duckdb-archive --format-version 1",
    {
      encoding: "utf8",
      maxBuffer: 100 * 1024 * 1024,
    },
  );
  const metadata: CargoMetadata = JSON.parse(metadataJson);

  // Keep only crates reachable from a workspace member through a "normal"
  // dependency edge (i.e. code that actually ships in the built binary).
  // This must be derived from the resolved dependency graph's edge kinds
  // (resolve.nodes[].deps[].dep_kinds), not from a crate's own target kinds:
  // almost every crate declares "test"/"example"/"custom-build" targets for
  // itself regardless of how *we* depend on it, so filtering on target kind
  // misclassifies any dependency that merely ships its own tests/examples/
  // build script alongside its library as "build/test only".
  const nodesById = new Map(
    metadata.resolve.nodes.map((node) => [node.id, node]),
  );
  const workspaceMemberIds = new Set(metadata.workspace_members);

  const reachableIds = new Set<string>();
  const visit = (id: string) => {
    if (reachableIds.has(id)) return;
    reachableIds.add(id);
    for (const dep of nodesById.get(id)?.deps ?? []) {
      const isNormal = dep.dep_kinds.some(
        (dk) => dk.kind === null || dk.kind === "normal",
      );
      if (isNormal) visit(dep.pkg);
    }
  };
  for (const memberId of workspaceMemberIds) visit(memberId);

  // Exclude the workspace members themselves: they are the app, not a
  // third-party notice.
  const runtimeCrates = new Set<string>();
  for (const pkg of metadata.packages) {
    if (reachableIds.has(pkg.id) && !workspaceMemberIds.has(pkg.id)) {
      runtimeCrates.add(`${pkg.name}@${pkg.version}`);
    }
  }

  const packageMap: Record<string, string> = {};
  for (const pkg of metadata.packages) {
    packageMap[`${pkg.name}@${pkg.version}`] = pkg.manifest_path
      .replace(/\\/g, "/")
      .replace(/\/Cargo.toml$/, "");
  }

  for (const crate of cargoData) {
    const crateKey = `${crate.name}@${crate.version}`;
    if (!runtimeCrates.has(crateKey)) continue; // Exclude build/test only crates

    output += generateLicenseTxt(crate.name, crate.license, crate.repository);

    // Find LICENSE file and add its contents
    const cratePath = packageMap[crateKey];
    if (cratePath) {
      const licenseFiles = [
        "LICENSE",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "COPYING",
      ];
      for (const file of licenseFiles) {
        const licensePath = path.join(cratePath, file);
        if (existsSync(licensePath)) {
          const licenseContent = readFileSync(licensePath, "utf8");
          output += "```LICENSE\n";
          output += `${licenseContent.trim().replace(/```/g, "`` ``` ``")}\n`;
          output += "```\n\n";
          break;
        }
      }
    }
  }
} catch (e) {
  console.error("❌ Failed to collect Rust licenses:", e);
  throw e;
}

const manualDir = path.resolve("./docs/licenses/manual");

/**
 * Append manual notices from the manual directory.
 *
 * @returns {string} A string containing the concatenated manual notices, or an empty string if none exist.
 */
const appendManualNotices = () => {
  if (!existsSync(manualDir)) return "";

  const files = readdirSync(manualDir)
    .filter((f) => f.endsWith(".md"))
    .sort();

  if (files.length === 0) return "";

  let s = "";
  for (const f of files) {
    const p = path.join(manualDir, f);
    const content = readFileSync(p, "utf8").trim();
    s += `${content}\n\n`;
  }
  return s;
};

// ==========================
// Output
// ==========================
if (!existsSync(outputDir)) {
  mkdirSync(outputDir, { recursive: true });
}

output += appendManualNotices();

writeFileSync(outputPath, output, "utf8");
console.log(`✅ Combined license file written to ${outputPath}`);
