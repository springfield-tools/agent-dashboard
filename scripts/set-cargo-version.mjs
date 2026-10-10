import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const PACKAGE_NAME = 'agent-dashboard';

export function setCargoTomlVersion(content, version) {
  if (!/^version\s*=\s*"[^"]+"/m.test(content)) {
    throw new Error('Failed to find version field in Cargo.toml');
  }

  return content.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
}

export function setCargoLockPackageVersion(content, version) {
  const pattern = new RegExp(
    `\\[\\[package\\]\\](\\r?\\n)name = "${PACKAGE_NAME}"\\1version = "[^"]+"`,
  );

  if (!pattern.test(content)) {
    throw new Error(`Failed to find ${PACKAGE_NAME} package in Cargo.lock`);
  }

  return content.replace(
    pattern,
    `[[package]]$1name = "${PACKAGE_NAME}"$1version = "${version}"`,
  );
}

function writeVersion(path, transform, version) {
  const content = readFileSync(path, 'utf8');
  writeFileSync(path, transform(content, version));
}

function main() {
  const version = process.argv[2];

  if (!version) {
    console.error('Usage: node scripts/set-cargo-version.mjs <version>');
    process.exit(1);
  }

  try {
    writeVersion(resolve(root, 'src-tauri/Cargo.toml'), setCargoTomlVersion, version);
    writeVersion(resolve(root, 'src-tauri/Cargo.lock'), setCargoLockPackageVersion, version);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (isCli) {
  main();
}
