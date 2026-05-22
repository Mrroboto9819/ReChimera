#!/usr/bin/env node
/*
 * Version sync + channel prep.
 *
 * Default mode (no flag): `Cargo.toml` workspace.package.version is
 * canonical. The script reads it and propagates the same version into
 * apps/desktop/package.json and apps/desktop/src-tauri/tauri.conf.json.
 *
 * --check         Dry-run; exits non-zero if anything would change.
 *                 Used by CI to make "forgot to sync" a build-time error.
 *
 * --canary        Puts the working tree into canary build mode locally,
 *                 mirroring what .github/workflows/release.yml does for
 *                 pushes to the develop branch. Specifically:
 *
 *                   - Computes a canary version `${base}-canary.${suffix}`
 *                     where `base` is the Cargo.toml version and `suffix`
 *                     defaults to `git rev-list --count HEAD` (a monotonic
 *                     integer — the MSI bundler requires the pre-release
 *                     identifier to be numeric and <= 65535, which rules
 *                     out hex SHAs).
 *                   - Patches apps/desktop/src-tauri/tauri.conf.json:
 *                       version, productName, identifier, updater endpoint.
 *                     Does NOT touch Cargo.toml or package.json (matches CI,
 *                     keeps the cargo cache hot).
 *                   - Patches the default brandColor in
 *                     apps/desktop/src/store.ts from the stable red to
 *                     the canary yellow.
 *                   - Regenerates the bundled icon set in
 *                     apps/desktop/src-tauri/icons/ from
 *                     apps/desktop/icon_canary.png by shelling out to
 *                     `bun tauri icon`.
 *
 *                 To undo: `git checkout -- apps/desktop/src-tauri/{tauri.conf.json,icons} apps/desktop/src/store.ts`.
 *
 * --suffix=<n>    Overrides the auto-detected suffix with a custom integer.
 *                 Must be a non-negative number <= 65535 (MSI bundle limit).
 *                 Useful for reproducing a specific canary build locally
 *                 or pinning a friendly version like `--suffix=99`.
 *                 Implies --canary.
 *
 * Usage:
 *   node scripts/sync-version.mjs                     # stable sync
 *   node scripts/sync-version.mjs --check             # CI gate
 *   node scripts/sync-version.mjs --canary            # canary prep
 *   node scripts/sync-version.mjs --canary --suffix=42
 */

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const suffixArg = args.find((a) => a.startsWith("--suffix="));
const canary = args.includes("--canary") || suffixArg != null;

const cargoPath = resolve(repoRoot, "Cargo.toml");
const pkgPath = resolve(repoRoot, "apps/desktop/package.json");
const tauriPath = resolve(repoRoot, "apps/desktop/src-tauri/tauri.conf.json");
const storePath = resolve(repoRoot, "apps/desktop/src/store.ts");
const canaryIconPath = resolve(repoRoot, "apps/desktop/icon_canary.png");

const CANARY_PRODUCT_NAME = "ReChimera Canary";
const CANARY_IDENTIFIER = "dev.rechimera.desktop.canary";
const CANARY_BRAND_COLOR = "#fcd202";

function readCargoVersion() {
  const cargoText = readFileSync(cargoPath, "utf-8");
  const match = cargoText.match(
    /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/m,
  );
  if (!match) {
    console.error(
      `[sync-version] could not find workspace.package.version in ${cargoPath}`,
    );
    process.exit(1);
  }
  return match[1];
}

if (canary) {
  runCanary();
} else {
  runStableSync();
}

function runStableSync() {
  const canonicalVersion = readCargoVersion();
  console.log(`[sync-version] canonical version (Cargo.toml): ${canonicalVersion}`);

  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
  let pkgChanged = false;
  if (pkg.version !== canonicalVersion) {
    console.log(`[sync-version] package.json: ${pkg.version} → ${canonicalVersion}`);
    pkg.version = canonicalVersion;
    pkgChanged = true;
  } else {
    console.log("[sync-version] package.json: in sync");
  }

  const tauri = JSON.parse(readFileSync(tauriPath, "utf-8"));
  let tauriChanged = false;
  if (tauri.version !== canonicalVersion) {
    console.log(`[sync-version] tauri.conf.json: ${tauri.version} → ${canonicalVersion}`);
    tauri.version = canonicalVersion;
    tauriChanged = true;
  } else {
    console.log("[sync-version] tauri.conf.json: in sync");
  }

  if (checkOnly) {
    if (pkgChanged || tauriChanged) {
      console.error(
        `\n[sync-version] FAIL — versions are out of sync. Run \`node scripts/sync-version.mjs\` and commit the result.`,
      );
      process.exit(1);
    }
    console.log("\n[sync-version] OK — all sources match Cargo.toml");
    process.exit(0);
  }

  // Preserve trailing newline so editors / formatters don't fight us each
  // time the file is rewritten.
  if (pkgChanged) writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
  if (tauriChanged) writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + "\n");
  console.log("\n[sync-version] done.");
}

function runCanary() {
  if (checkOnly) {
    console.error(
      "[sync-version] --check is not supported with --canary (canary state isn't a stable invariant — re-run --canary to refresh).",
    );
    process.exit(1);
  }

  const baseVersion = readCargoVersion();
  const suffix = resolveSuffix();
  const canaryVersion = `${baseVersion}-canary.${suffix}`;

  console.log(`[sync-version] canary mode`);
  console.log(`[sync-version]   base version : ${baseVersion}  (Cargo.toml)`);
  console.log(`[sync-version]   suffix       : ${suffix}`);
  console.log(`[sync-version]   canary ver.  : ${canaryVersion}`);

  patchTauriConfForCanary(canaryVersion);
  patchStoreBrandColor();
  regenerateCanaryIcons();

  console.log(`\n[sync-version] canary prep done.`);
  console.log(
    `[sync-version] to revert: git checkout -- apps/desktop/src-tauri/{tauri.conf.json,icons} apps/desktop/src/store.ts`,
  );
}

function resolveSuffix() {
  // The MSI bundle target requires the version's pre-release identifier to
  // be numeric-only and <= 65535. That rules out hex SHAs. Default auto-mode
  // uses `git rev-list --count HEAD` (monotonic integer that increments on
  // every commit — deterministic for a given commit, so re-running --canary
  // at the same HEAD is idempotent). The --suffix= escape hatch still
  // accepts any numeric value the user wants to pin.
  if (suffixArg) {
    const value = suffixArg.slice("--suffix=".length).trim();
    if (!value) {
      console.error("[sync-version] --suffix= was passed with no value");
      process.exit(1);
    }
    if (!/^\d+$/.test(value)) {
      console.error(
        `[sync-version] suffix "${value}" must be numeric-only (MSI bundle requirement). Try a number like --suffix=42.`,
      );
      process.exit(1);
    }
    const numeric = Number(value);
    if (numeric > 65535) {
      console.error(
        `[sync-version] suffix ${value} exceeds the MSI cap of 65535. Pick a smaller number.`,
      );
      process.exit(1);
    }
    return value;
  }
  const r = spawnSync("git", ["rev-list", "--count", "HEAD"], {
    cwd: repoRoot,
    encoding: "utf-8",
  });
  if (r.status !== 0) {
    console.error(
      "[sync-version] could not auto-detect commit count via `git rev-list --count HEAD` — pass --suffix=<numeric value> instead.",
    );
    if (r.stderr) console.error(r.stderr.trim());
    process.exit(1);
  }
  const count = r.stdout.trim();
  if (Number(count) > 65535) {
    console.error(
      `[sync-version] commit count ${count} exceeds the MSI cap of 65535. Pick a custom --suffix=<n> instead.`,
    );
    process.exit(1);
  }
  return count;
}

function patchTauriConfForCanary(canaryVersion) {
  const tauri = JSON.parse(readFileSync(tauriPath, "utf-8"));

  // Updater endpoint: derive from whatever the existing endpoint is so the
  // script works on forks that already point the stable endpoint at their
  // own GitHub repo. We only require the prefix
  // `https://github.com/<owner>/<repo>/` and we swap the rest of the path.
  const existingEndpoints = tauri.plugins?.updater?.endpoints ?? [];
  if (existingEndpoints.length === 0) {
    console.error(
      "[sync-version] tauri.conf.json has no plugins.updater.endpoints[0] — can't derive the canary endpoint",
    );
    process.exit(1);
  }
  const repoMatch = existingEndpoints[0].match(
    /^(https:\/\/github\.com\/[^/]+\/[^/]+)\//,
  );
  if (!repoMatch) {
    console.error(
      `[sync-version] could not parse the GitHub repo out of endpoint "${existingEndpoints[0]}"`,
    );
    process.exit(1);
  }
  const canaryEndpoint = `${repoMatch[1]}/releases/download/canary-latest/latest.json`;

  tauri.version = canaryVersion;
  tauri.productName = CANARY_PRODUCT_NAME;
  tauri.identifier = CANARY_IDENTIFIER;
  tauri.plugins = tauri.plugins ?? {};
  tauri.plugins.updater = tauri.plugins.updater ?? {};
  tauri.plugins.updater.endpoints = [canaryEndpoint];

  writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + "\n");
  console.log(`[sync-version] tauri.conf.json patched`);
  console.log(`[sync-version]   productName  → ${CANARY_PRODUCT_NAME}`);
  console.log(`[sync-version]   identifier   → ${CANARY_IDENTIFIER}`);
  console.log(`[sync-version]   updater endp → ${canaryEndpoint}`);
}

function patchStoreBrandColor() {
  const s = readFileSync(storePath, "utf-8");
  const next = s.replace(
    /brandColor:\s*"#[0-9a-fA-F]{6}"/,
    `brandColor: "${CANARY_BRAND_COLOR}"`,
  );
  if (next === s) {
    // Two reasons this can happen: (a) brandColor literal was renamed and
    // the regex is stale, or (b) the working tree is already in canary
    // mode (same value already there). Treat as soft warning so the
    // script is idempotent.
    if (s.includes(`brandColor: "${CANARY_BRAND_COLOR}"`)) {
      console.log(`[sync-version] store.ts brandColor already ${CANARY_BRAND_COLOR}`);
      return;
    }
    console.error(
      "[sync-version] brandColor literal not found in apps/desktop/src/store.ts — regex may be stale",
    );
    process.exit(1);
  }
  writeFileSync(storePath, next);
  console.log(`[sync-version] store.ts brandColor → ${CANARY_BRAND_COLOR}`);
}

function regenerateCanaryIcons() {
  if (!existsSync(canaryIconPath)) {
    console.error(
      `[sync-version] canary icon source not found at ${canaryIconPath}`,
    );
    process.exit(1);
  }
  console.log(`[sync-version] regenerating icon set from icon_canary.png …`);
  const r = spawnSync("bun", ["tauri", "icon", canaryIconPath], {
    cwd: resolve(repoRoot, "apps/desktop"),
    stdio: "inherit",
  });
  if (r.status !== 0) {
    console.error(
      `[sync-version] bun tauri icon failed with exit code ${r.status}`,
    );
    process.exit(r.status ?? 1);
  }
}
