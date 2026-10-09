// Moves the game's version on by one step and writes it everywhere it is kept: package.json, the Cargo
// workspace and the workspace's own entries in Cargo.lock. Prints the new version. The release workflow
// (.github/workflows/release.yml) runs it; it can also be run by hand.
//
//   node tools/release/bump-version.mjs minor     # 0.3.1 -> 0.4.0 (also: major, patch, or an exact 1.2.3)

import { readFileSync, writeFileSync } from "node:fs";

const step = process.argv[2] ?? "minor";
const root = new URL("../../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");
const write = (path, text) => writeFileSync(new URL(path, root), text);

const pkg = read("package.json");
const current = JSON.parse(pkg).version;
const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
if (!match) throw new Error(`package.json has a version this script cannot step: ${current}`);
const [major, minor, patch] = match.slice(1).map(Number);

let next;
if (step === "major") next = `${major + 1}.0.0`;
else if (step === "minor") next = `${major}.${minor + 1}.0`;
else if (step === "patch") next = `${major}.${minor}.${patch + 1}`;
else if (/^\d+\.\d+\.\d+$/.test(step)) next = step;
else throw new Error(`Expected major, minor, patch or a version like 1.2.3, not ${step}`);

// Each file must change exactly where expected, so a layout this script does not know fails loudly instead of
// releasing with two versions in the tree.
function replace(path, text, pattern, replacement, count) {
  let n = 0;
  const out = text.replace(pattern, (...m) => {
    n += 1;
    return replacement(...m);
  });
  if (n !== count) throw new Error(`${path}: expected ${count} version to change, found ${n}`);
  write(path, out);
}

replace("package.json", pkg, /^(\s*"version": )"[^"]*"/m, (_, head) => `${head}"${next}"`, 1);
const cargo = read("server/Cargo.toml");
replace(
  "server/Cargo.toml",
  cargo,
  /(\[workspace\.package\][^[]*?\nversion = )"[^"]*"/,
  (_, head) => `${head}"${next}"`,
  1,
);
// The workspace's crates are the lock entries named fsw-* with no `source` line (they are path packages).
const lock = read("server/Cargo.lock");
const crates = (lock.match(/^name = "fsw-[^"]+"\nversion = "[^"]*"\n(?!source)/gm) ?? []).length;
if (crates === 0) throw new Error("server/Cargo.lock: no fsw-* crates found");
replace(
  "server/Cargo.lock",
  lock,
  /^(name = "fsw-[^"]+"\nversion = )"[^"]*"\n(?!source)/gm,
  (_, head) => `${head}"${next}"\n`,
  crates,
);

console.log(next);
