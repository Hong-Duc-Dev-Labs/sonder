#!/usr/bin/env node
// Writes the updater's latest.json for installers built on this machine, for releases made
// without the release workflow. Run after `pnpm tauri build` with TAURI_SIGNING_PRIVATE_KEY
// set, so every installer has its .sig beside it:
//
//   node scripts/updater-manifest.mjs <tag> [--notes notes.md] [--merge latest.json] > out.json
//
// Building on several OSes: run it on each one, passing the previous output as --merge, so
// one latest.json lists every platform. The files to attach to the release (each installer
// and its .sig) are printed to stderr as `attach: <path>` lines.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "Hong-Duc-Dev-Labs/sonder";
const BUNDLE = fileURLToPath(new URL("../target/release/bundle/", import.meta.url));

function fail(message) {
  console.error(message);
  process.exit(1);
}

const args = process.argv.slice(2);
const tag = args.shift();
const options = {};
while (args.length) {
  const flag = args.shift();
  if (flag !== "--notes" && flag !== "--merge") fail(`unknown option ${flag}`);
  options[flag.slice(2)] = args.shift() ?? fail(`${flag} needs a file`);
}
if (!tag || !/^v\d+\.\d+\.\d+/.test(tag)) {
  fail("usage: node scripts/updater-manifest.mjs <tag like v0.1.0> [--notes file] [--merge latest.json]");
}
const version = tag.slice(1);

const mac = process.arch === "arm64" ? "darwin-aarch64" : "darwin-x86_64";
// The installer kind behind each platform key the updater asks for. A key listed under
// several kinds takes the first one found, so NSIS stays the default on Windows.
const kinds = [
  { dir: "nsis", suffix: "-setup.exe", keys: ["windows-x86_64", "windows-x86_64-nsis"] },
  { dir: "msi", suffix: ".msi", keys: ["windows-x86_64", "windows-x86_64-msi"] },
  { dir: "macos", suffix: ".app.tar.gz", keys: [mac, `${mac}-app`] },
  { dir: "appimage", suffix: ".AppImage", keys: ["linux-x86_64", "linux-x86_64-appimage"] },
  { dir: "deb", suffix: ".deb", keys: ["linux-x86_64-deb"] },
  { dir: "rpm", suffix: ".rpm", keys: ["linux-x86_64-rpm"] },
];
// Older builds stay in the bundle folders; only take this version's installers. The macOS
// archive carries no version in its name.
const isThisVersion = (dir, file) =>
  dir === "macos" || file.includes(`_${version}_`) || file.includes(`-${version}-`);

const base = options.merge ? JSON.parse(readFileSync(options.merge, "utf8")) : null;
if (base && base.version !== version) fail(`${options.merge} is for ${base.version}, not ${version}`);

const platforms = {};
const attach = [];
for (const { dir, suffix, keys } of kinds) {
  const path = join(BUNDLE, dir);
  if (!existsSync(path)) continue;
  const files = readdirSync(path).filter((f) => f.endsWith(suffix) && isThisVersion(dir, f));
  if (files.length > 1) fail(`more than one ${suffix} for ${version} in ${path}`);
  for (const file of files) {
    const sig = join(path, `${file}.sig`);
    if (!existsSync(sig)) fail(`${file} has no .sig; build with TAURI_SIGNING_PRIVATE_KEY set`);
    attach.push(join(path, file), sig);
    const entry = {
      signature: readFileSync(sig, "utf8").trim(),
      // GitHub stores asset names with spaces as dots.
      url: `https://github.com/${REPO}/releases/download/${tag}/${encodeURIComponent(file.replaceAll(" ", "."))}`,
    };
    for (const key of keys) platforms[key] ??= entry;
  }
}
if (attach.length === 0) fail("no installers for this version under target/release/bundle; run `pnpm tauri build` first");

const manifest = {
  version,
  notes: options.notes ? readFileSync(options.notes, "utf8").trim() : (base?.notes ?? ""),
  pub_date: base?.pub_date ?? new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: { ...base?.platforms, ...platforms },
};
for (const file of attach) console.error(`attach: ${file}`);
process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
