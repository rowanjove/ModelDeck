const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execSync } = require('child_process');
const { checkVersions } = require('./check-version-consistency.cjs');

function getSha256(filePath) {
  const buf = fs.readFileSync(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex').toLowerCase();
}

function formatBytes(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
}

// 检查版本一致性
const versionCheck = checkVersions();
if (!versionCheck.success) {
  console.error('[Error] Version consistency check failed. Aborting portable release packaging.');
  process.exit(1);
}
const version = versionCheck.version;

const rootDir = path.resolve(__dirname, '..');
const srcExe = path.join(rootDir, 'src-tauri', 'target', 'release', 'ModelDeck.exe');
const targetDir = path.join(rootDir, 'release-dist', version);
const portableDirName = `ModelDeck-${version}-Portable`;
const portableDir = path.join(targetDir, portableDirName);
const portableExeName = `ModelDeck-${version}-Portable.exe`;
const portableExe = path.join(targetDir, portableExeName);
const portableZipName = `ModelDeck-${version}-Portable.zip`;
const portableZip = path.join(targetDir, portableZipName);

if (!fs.existsSync(srcExe)) {
  console.error(`Source exe not found: ${srcExe}`);
  console.error('Please run `npm run tauri build -- --no-bundle` or `cargo build --release --manifest-path src-tauri/Cargo.toml` first.');
  process.exit(1);
}

fs.mkdirSync(portableDir, { recursive: true });

// 1. Copy exe
fs.copyFileSync(srcExe, portableExe);
fs.copyFileSync(srcExe, path.join(portableDir, 'ModelDeck.exe'));

// 2. Zip folder via PowerShell Compress-Archive
console.log(`Compressing portable package (${portableZipName})...`);
if (fs.existsSync(portableZip)) {
  fs.unlinkSync(portableZip);
}
execSync(`powershell -Command "Compress-Archive -Path '${portableDir}' -DestinationPath '${portableZip}' -Force"`);

// 3. Stats & SHA256
const statExe = fs.statSync(portableExe);
const statZip = fs.statSync(portableZip);
const hashExe = getSha256(portableExe);
const hashZip = getSha256(portableZip);

console.log('=== Portable Release Artifacts ===');
console.log(`Executable: ${portableExe}`);
console.log(`  Size:     ${formatBytes(statExe.size)} (${statExe.size} bytes)`);
console.log(`  SHA256:   ${hashExe}`);
console.log(`Zip Archive: ${portableZip}`);
console.log(`  Size:     ${formatBytes(statZip.size)} (${statZip.size} bytes)`);
console.log(`  SHA256:   ${hashZip}`);

// 4. Update SHA256SUMS.txt and checksums.sha256
const sumsFiles = [
  path.join(targetDir, 'SHA256SUMS.txt'),
  path.join(targetDir, 'checksums.sha256'),
];

for (const sumsFile of sumsFiles) {
  let sums = '';
  if (fs.existsSync(sumsFile)) {
    sums = fs.readFileSync(sumsFile, 'utf8');
  }

  const lines = sums.split('\n').filter(line => {
    const trimmed = line.trim();
    return trimmed && !trimmed.endsWith(portableExeName) && !trimmed.endsWith(portableZipName);
  });

  lines.push(`${hashExe}  ${portableExeName}`);
  lines.push(`${hashZip}  ${portableZipName}`);

  fs.writeFileSync(sumsFile, lines.join('\n') + '\n', 'utf8');
  console.log(`Updated: ${sumsFile}`);
}
