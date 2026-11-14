#!/usr/bin/env node
// Writes the updater's latest.json for installers built on this machine, for releases made
// without the release workflow. Run after `pnpm tauri build` with TAURI_SIGNING_PRIVATE_KEY
// set, so every installer has its .sig beside it:
//
//   node scripts/updater-manifest.mjs <tag> [notes-file] > latest.json
//
// Then attach latest.json, the installers and their .sig files to the GitHub release <tag>.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "Hong-Duc-Dev-Labs/sonder";
const BUNDLE = fileURLToPath(new URL("../target/release/bundle/", import.meta.url));

const [tag, notesFile] = process.argv.slice(2);
if (!tag || !/^v\d+\.\d+\.\d+/.test(tag)) {
  console.error("usage: node scripts/updater-manifest.mjs <tag like v0.1.0> [notes-file]");
  process.exit(2);
}

// The installer kind the updater runs on each platform key it asks for.
const kinds = [
  { dir: "nsis", suffix: "-setup.exe", keys: ["windows-x86_64", "windows-x86_64-nsis"] },
  { dir: "msi", suffix: ".msi", keys: ["windows-x86_64-msi"] },
  { dir: "macos", suffix: ".app.tar.gz", keys: ["darwin-aarch64", "darwin-aarch64-app"] },
  { dir: "appimage", suffix: ".AppImage", keys: ["linux-x86_64", "linux-x86_64-appimage"] },
];

const version = tag.replace(/^v/, "");
// Older builds stay in the bundle folders; only take this version's installers. The macOS
// archive carries no version in its name.
const isThisVersion = (dir, file) => dir === "macos" || file.includes(`_${version}_`);

const platforms = {};
for (const { dir, suffix, keys } of kinds) {
  const path = join(BUNDLE, dir);
  if (!existsSync(path)) continue;
  for (const file of readdirSync(path).filter((f) => f.endsWith(suffix) && isThisVersion(dir, f))) {
    const sig = join(path, `${file}.sig`);
    if (!existsSync(sig)) {
      console.error(`${file} has no .sig; build with TAURI_SIGNING_PRIVATE_KEY set`);
      process.exit(1);
    }
    const entry = {
      signature: readFileSync(sig, "utf8").trim(),
      url: `https://github.com/${REPO}/releases/download/${tag}/${encodeURIComponent(file)}`,
    };
    // The first kind listed for a key wins, so NSIS stays the Windows default.
    for (const key of keys) platforms[key] ??= entry;
  }
}

if (Object.keys(platforms).length === 0) {
  console.error("no signed installers under target/release/bundle; run `pnpm tauri build` first");
  process.exit(1);
}

const manifest = {
  version,
  notes: notesFile ? readFileSync(notesFile, "utf8").trim() : "",
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms,
};
process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
