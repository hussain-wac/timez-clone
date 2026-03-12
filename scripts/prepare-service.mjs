import { copyFileSync, chmodSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';

const rootDir = resolve(dirname(new URL(import.meta.url).pathname), '..');
const srcTauriDir = join(rootDir, 'src-tauri');
const resourcesDir = join(srcTauriDir, 'resources');
const profile = process.argv.includes('--release') ? 'release' : 'debug';
const cargoArgs = [
  'build',
  '-p',
  'timez-service',
  '--manifest-path',
  join(srcTauriDir, 'Cargo.toml'),
  '--offline',
];

if (profile === 'release') {
  cargoArgs.push('--release');
}

execFileSync('cargo', cargoArgs, {
  cwd: srcTauriDir,
  stdio: 'inherit',
});

const binaryName = process.platform === 'win32' ? 'timez-service.exe' : 'timez-service';
const builtBinary = join(srcTauriDir, 'target', profile, binaryName);
const bundledBinary = join(resourcesDir, binaryName);

mkdirSync(resourcesDir, { recursive: true });
copyFileSync(builtBinary, bundledBinary);

if (process.platform !== 'win32') {
  chmodSync(bundledBinary, 0o755);
}
