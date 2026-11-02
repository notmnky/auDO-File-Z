// Fails if the app version differs between the four files that carry it,
// or (when a tag is given) between those files and the release tag.
//
//   node scripts/check-versions.js            # files only
//   node scripts/check-versions.js v0.1.9005  # files and tag
//
// Why: the running app compares version.json with the latest release tag.
// If they disagree, users see an update that never goes away, or never see one.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => fs.readFileSync(path.join(root, rel), "utf8");

const cargoVersion = /^\s*version\s*=\s*"([^"]+)"/m.exec(read("src-tauri/Cargo.toml"))?.[1];

const versions = {
  "version.json": JSON.parse(read("version.json")).version,
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "src-tauri/Cargo.toml": cargoVersion,
};

const expected = versions["version.json"];
let ok = true;

for (const [file, v] of Object.entries(versions)) {
  if (v !== expected) {
    console.error(`✗ ${file} has version ${v ?? "(missing)"}, expected ${expected} (from version.json)`);
    ok = false;
  }
}

const tag = process.argv[2];
if (tag) {
  if (tag !== `v${expected}`) {
    console.error(`✗ Tag ${tag} does not match the app version. Expected tag v${expected}.`);
    ok = false;
  }
}

if (!ok) process.exit(1);
console.log(`✓ All versions are ${expected}${tag ? ` and match tag ${tag}` : ""}.`);
