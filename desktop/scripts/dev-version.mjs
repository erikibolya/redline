#!/usr/bin/env node
// Stamp a dev version into the Cargo manifests, lockfiles and the Tauri dev config.
//
// The browser image is built from `desktop/Cargo.toml`; without this the container
// label says `0.27.0-dev.N` while the binary inside still reports the stable
// `0.26.1` (2026-11-01). The Windows dev job needs the same stamp for its CLI and
// agent assets. Keeping the lockfile packages in sync also keeps `--locked` builds
// working.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+-dev\.\d+$/.test(version)) {
  console.error('usage: node desktop/scripts/dev-version.mjs <major.minor.patch-dev.n>');
  process.exit(2);
}

const root = fileURLToPath(new URL('../../', import.meta.url));
const read = (path) => readFileSync(join(root, path), 'utf8');
const write = (path, text) => writeFileSync(join(root, path), text);

function setOnce(path, regex, what) {
  const text = read(path);
  if (!regex.test(text)) {
    throw new Error(`cannot find ${what} in ${path}`);
  }
  write(path, text.replace(regex, `$1${version}$2`));
}

function setLockVersions(path, names) {
  let text = read(path);
  for (const name of names) {
    const regex = new RegExp(
      `(\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = ")[^"]+(")`,
    );
    if (!regex.test(text)) {
      throw new Error(`cannot find package ${name} in ${path}`);
    }
    text = text.replace(regex, `$1${version}$2`);
  }
  write(path, text);
}

setOnce(
  'desktop/Cargo.toml',
  /(\[workspace\.package\][\s\S]*?^version = ")[^"]+(")/m,
  'workspace package version',
);
setOnce(
  'desktop/app/Cargo.toml',
  /(\[package\][\s\S]*?^version = ")[^"]+(")/m,
  'app package version',
);

setLockVersions('desktop/Cargo.lock', [
  'redline',
  'redline-agent',
  'redline-core',
]);
if (existsSync(join(root, 'desktop/app/Cargo.lock'))) {
  setLockVersions('desktop/app/Cargo.lock', [
    'redline-app',
    'redline-agent',
    'redline-core',
  ]);
}

write('desktop/app/tauri.dev.conf.json', JSON.stringify({ version }) + '\n');
console.log(`stamped dev version ${version}`);
