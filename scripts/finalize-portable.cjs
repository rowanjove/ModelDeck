const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execSync } = require('child_process');

function getSha256(filePath) {
  const buf = fs.readFileSync(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex').toLowerCase();
}

function formatBytes(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
}

const rootDir = path.resolve(__dirname, '..');
const srcExe = path.join(rootDir, 'src-tauri', 'target', 'release', 'ModelDeck.exe');
const targetDir = path.join(rootDir, 'release-dist', '1.0.0');
const portableDir = path.join(targetDir, 'ModelDeck-1.0.0-Portable');
const portableExe = path.join(targetDir, 'ModelDeck-1.0.0-Portable.exe');
const portableZip = path.join(targetDir, 'ModelDeck-1.0.0-Portable.zip');

if (!fs.existsSync(srcExe)) {
  console.error(`Source exe not found: ${srcExe}`);
  process.exit(1);
}

fs.mkdirSync(portableDir, { recursive: true });

// 1. Copy exe
fs.copyFileSync(srcExe, portableExe);
fs.copyFileSync(srcExe, path.join(portableDir, 'ModelDeck.exe'));

// 2. Zip folder via PowerShell Compress-Archive
console.log('Compressing portable package...');
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

// 4. Update SHA256SUMS.txt
const sumsFile = path.join(targetDir, 'SHA256SUMS.txt');
let sums = '';
if (fs.existsSync(sumsFile)) {
  sums = fs.readFileSync(sumsFile, 'utf8');
}

const lines = sums.split('\n').filter(line => {
  const trimmed = line.trim();
  return trimmed && !trimmed.endsWith('ModelDeck-1.0.0-Portable.exe') && !trimmed.endsWith('ModelDeck-1.0.0-Portable.zip');
});

lines.push(`${hashExe}  ModelDeck-1.0.0-Portable.exe`);
lines.push(`${hashZip}  ModelDeck-1.0.0-Portable.zip`);

fs.writeFileSync(sumsFile, lines.join('\n') + '\n', 'utf8');
console.log(`Updated: ${sumsFile}`);
