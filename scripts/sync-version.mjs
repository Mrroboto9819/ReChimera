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
 *                 pushes to the develop branch. Touches the five surfaces
 *                 the channel identity flows through:
 *
 *                   1. tauri.conf.json
 *                        version + productName + window title + identifier
 *                        + updater endpoint
 *                   2. package.json
 *                        version (Vite reads this at build time for
 *                        APP_VERSION shown in the title bar)
 *                   3. apps/desktop/src/store.ts
 *                        default brandColor flips from red to yellow
 *                   4. apps/desktop/src/App.tsx
 *                        brand text literal swaps to "ReChimera Canary"
 *                   5. apps/desktop/icon.png
 *                        overwritten with icon_canary.png (Vite bundles
 *                        this as the frontend brand icon)
 *                   6. apps/desktop/src-tauri/icons/
 *                        OS-level binary icon set regenerated from
 *                        icon_canary.png via `bun tauri icon`
 *
 *                 Cargo.toml is NOT touched (so the Cargo cache stays hot).
 *
 *                 The canary version is `${base}-${suffix}` where `base` is
 *                 the Cargo.toml version and `suffix` defaults to
 *                 `git rev-list --count HEAD` (a monotonic integer). The
 *                 MSI bundler requires the SemVer pre-release portion to be
 *                 a single numeric value <= 65535 — no words, no dots — so
 *                 the "canary" word is conveyed by the productName /
 *                 identifier / brand color / icon, not by the version string.
 *
 *                 To undo every patch:
 *                   git checkout -- \
 *                     apps/desktop/package.json \
 *                     apps/desktop/icon.png \
 *                     apps/desktop/src/App.tsx \
 *                     apps/desktop/src/store.ts \
 *                     apps/desktop/src-tauri/tauri.conf.json \
 *                     apps/desktop/src-tauri/icons
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

import { readFileSync, writeFileSync, existsSync, copyFileSync } from "node:fs";
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
const appTsxPath = resolve(repoRoot, "apps/desktop/src/App.tsx");
const frontendIconPath = resolve(repoRoot, "apps/desktop/icon.png");
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
  // MSI bundler requires the pre-release identifier to be a single numeric
  // value <= 65535 — no words, no dots. So the version stays `base-<n>`
  // (the "canary" identity is conveyed by productName / identifier / icon
  // / brand color patched below, not by the version string itself).
  const canaryVersion = `${baseVersion}-${suffix}`;

  console.log(`[sync-version] canary mode`);
  console.log(`[sync-version]   base version : ${baseVersion}  (Cargo.toml)`);
  console.log(`[sync-version]   suffix       : ${suffix}`);
  console.log(`[sync-version]   canary ver.  : ${canaryVersion}`);

  patchTauriConfForCanary(canaryVersion);
  patchPackageJsonVersion(canaryVersion);
  patchStoreBrandColor();
  patchAppTsxBrandText();
  swapFrontendIcon();
  regenerateCanaryIcons();

  console.log(`\n[sync-version] canary prep done.`);
  console.log(`[sync-version] to revert: git checkout -- \\`);
  console.log(`  apps/desktop/package.json \\`);
  console.log(`  apps/desktop/icon.png \\`);
  console.log(`  apps/desktop/src/App.tsx \\`);
  console.log(`  apps/desktop/src/store.ts \\`);
  console.log(`  apps/desktop/src-tauri/tauri.conf.json \\`);
  console.log(`  apps/desktop/src-tauri/icons`);
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
  // Window title shows in the OS taskbar / Alt+Tab — mirror productName so
  // the channel is visible there too, matching the CI workflow.
  if (Array.isArray(tauri.app?.windows) && tauri.app.windows[0]) {
    tauri.app.windows[0].title = CANARY_PRODUCT_NAME;
  }
  tauri.plugins = tauri.plugins ?? {};
  tauri.plugins.updater = tauri.plugins.updater ?? {};
  tauri.plugins.updater.endpoints = [canaryEndpoint];

  writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + "\n");
  console.log(`[sync-version] tauri.conf.json patched`);
  console.log(`[sync-version]   productName  → ${CANARY_PRODUCT_NAME}`);
  console.log(`[sync-version]   windowTitle  → ${CANARY_PRODUCT_NAME}`);
  console.log(`[sync-version]   identifier   → ${CANARY_IDENTIFIER}`);
  console.log(`[sync-version]   updater endp → ${canaryEndpoint}`);
}

function patchPackageJsonVersion(canaryVersion) {
  // APP_VERSION in apps/desktop/src/version.ts reads from package.json at
  // Vite build time (also injected as `__APP_VERSION__` via vite.config.ts's
  // define block). Without this patch the title bar shows the unpatched
  // base version (e.g. v0.4.0) instead of the canary version (v0.4.0-N).
  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
  if (pkg.version === canaryVersion) {
    console.log(`[sync-version] package.json version already ${canaryVersion}`);
    return;
  }
  pkg.version = canaryVersion;
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
  console.log(`[sync-version] package.json version → ${canaryVersion}`);
}

function patchAppTsxBrandText() {
  // App.tsx has a hardcoded "ReChimera" literal next to the brand icon
  // in the top-left of the title bar. Swap it to "ReChimera Canary" so the
  // user-visible brand text matches the channel.
  const s = readFileSync(appTsxPath, "utf-8");
  const next = s.replace(
    /(<img\s+src=\{brandIconUrl\}[\s\S]*?\/>\s*\n\s*)ReChimera(\s*\n\s*<span className="brand-version)/,
    `$1${CANARY_PRODUCT_NAME}$2`,
  );
  if (next === s) {
    if (s.includes(`            ${CANARY_PRODUCT_NAME}\n`)) {
      console.log(`[sync-version] App.tsx brand text already "${CANARY_PRODUCT_NAME}"`);
      return;
    }
    console.error(
      "[sync-version] brand text anchor not found in App.tsx — regex may be stale",
    );
    process.exit(1);
  }
  writeFileSync(appTsxPath, next);
  console.log(`[sync-version] App.tsx brand text → ${CANARY_PRODUCT_NAME}`);
}

function swapFrontendIcon() {
  // Vite bundles apps/desktop/icon.png as a static asset for the frontend
  // brand icon (title bar / About / Splash / Settings) via `?url` imports.
  // The src-tauri/icons/ regen below covers the OS-level binary icon, but
  // the frontend keeps showing the stable red icon until we overwrite
  // icon.png with the canary source.
  if (!existsSync(canaryIconPath)) {
    console.error(
      `[sync-version] canary icon source not found at ${canaryIconPath}`,
    );
    process.exit(1);
  }
  copyFileSync(canaryIconPath, frontendIconPath);
  console.log(`[sync-version] apps/desktop/icon.png ← icon_canary.png  (frontend icon)`);
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
