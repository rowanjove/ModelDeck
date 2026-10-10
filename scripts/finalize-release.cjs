const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execSync } = require('child_process');
const { checkVersions } = require('./check-version-consistency.cjs');

function getFileSha256(filePath) {
  const fileBuffer = fs.readFileSync(filePath);
  const hashSum = crypto.createHash('sha256');
  hashSum.update(fileBuffer);
  return hashSum.digest('hex').toLowerCase();
}

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

const rootDir = path.resolve(__dirname, '..');
const distWindowsDir = path.join(rootDir, 'dist', 'windows');
const optionalDir = path.join(distWindowsDir, 'optional');

// 先进行多端版本号一致性强校验
const versionCheck = checkVersions();
if (!versionCheck.success) {
  console.error('[Error] Version consistency check failed across configuration files! Aborting release finalize.');
  process.exit(1);
}
const version = versionCheck.version;

// 先清空 dist/windows 目录，确保无历史残留
if (fs.existsSync(distWindowsDir)) {
  fs.rmSync(distWindowsDir, { recursive: true, force: true });
}
fs.mkdirSync(optionalDir, { recursive: true });

const targetReleaseDir = path.join(rootDir, 'src-tauri', 'target', 'release');
const nsisBundleDir = path.join(targetReleaseDir, 'bundle', 'nsis');
const msiBundleDir = path.join(targetReleaseDir, 'bundle', 'msi');

// 1. Portable: src-tauri/target/release/ModelDeck.exe -> dist/windows/ModelDeck-Portable.exe
const sourcePortable = path.join(targetReleaseDir, 'ModelDeck.exe');
const targetPortable = path.join(distWindowsDir, 'ModelDeck-Portable.exe');
const targetVersionedPortable = path.join(distWindowsDir, `ModelDeck-${version}-Portable.exe`);

if (!fs.existsSync(sourcePortable)) {
  console.error(`Error: Portable binary not found at ${sourcePortable}`);
  console.error('Please build the release binary first (`npm run tauri build -- --no-bundle`).');
  process.exit(1);
}
fs.copyFileSync(sourcePortable, targetPortable);
fs.copyFileSync(sourcePortable, targetVersionedPortable);
console.log(`Copied: ModelDeck-Portable.exe -> ${targetPortable}`);
console.log(`Copied: ModelDeck-${version}-Portable.exe -> ${targetVersionedPortable}`);

// 2. Setup (NSIS): 匹配当前版本的 ModelDeck_${version}_x64-setup.exe -> dist/windows/ModelDeck-Setup.exe
const currentVersionSetup = path.join(nsisBundleDir, `ModelDeck_${version}_x64-setup.exe`);
const targetSetup = path.join(distWindowsDir, 'ModelDeck-Setup.exe');
const targetVersionedSetup = path.join(distWindowsDir, `ModelDeck-${version}-Setup.exe`);

if (fs.existsSync(currentVersionSetup)) {
  fs.copyFileSync(currentVersionSetup, targetSetup);
  fs.copyFileSync(currentVersionSetup, targetVersionedSetup);
  console.log(`Copied: ${path.basename(currentVersionSetup)} -> ${targetSetup}`);
} else if (fs.existsSync(nsisBundleDir)) {
  // fallback: 找最新的 .exe
  const files = fs.readdirSync(nsisBundleDir).filter(f => f.endsWith('.exe'));
  const sorted = files.map(f => ({ name: f, time: fs.statSync(path.join(nsisBundleDir, f)).mtimeMs }))
                      .sort((a, b) => b.time - a.time);
  if (sorted.length > 0) {
    fs.copyFileSync(path.join(nsisBundleDir, sorted[0].name), targetSetup);
    fs.copyFileSync(path.join(nsisBundleDir, sorted[0].name), targetVersionedSetup);
    console.log(`Copied (latest): ${sorted[0].name} -> ${targetSetup}`);
  } else {
    console.warn(`Warning: No NSIS setup found in ${nsisBundleDir}. Continuing with Portable-only.`);
  }
} else {
  console.warn(`Warning: NSIS bundle directory ${nsisBundleDir} not found.`);
}

// 3. Optional MSI: 复制当前版本的 MSI 文件
if (fs.existsSync(msiBundleDir)) {
  const msiFiles = fs.readdirSync(msiBundleDir).filter(f => f.includes(version) && f.endsWith('.msi'));
  for (const f of msiFiles) {
    const srcMsi = path.join(msiBundleDir, f);
    const destMsi = path.join(optionalDir, f);
    fs.copyFileSync(srcMsi, destMsi);
    console.log(`Copied optional MSI: ${f} -> ${destMsi}`);
  }
}

// 4. 统计汇总信息与校验和计算
let gitCommit = 'Uncommitted (Master head)';
try {
  gitCommit = execSync('git rev-parse HEAD', { cwd: rootDir }).toString().trim();
} catch (e) {
  try {
    const status = execSync('git status --short', { cwd: rootDir }).toString().trim();
    gitCommit = `Working Directory (${status.split('\n').length} modified/untracked files)`;
  } catch (_) {}
}

const buildTime = new Date().toISOString();

const artifacts = [];
const checksumLines = [];

function recordFile(p, relativePath) {
  if (fs.existsSync(p)) {
    const stat = fs.statSync(p);
    const sha256 = getFileSha256(p);
    artifacts.push({
      file: relativePath,
      sizeBytes: stat.size,
      sizeFormatted: formatBytes(stat.size),
      sha256,
      lastModified: stat.mtime.toISOString(),
    });
    checksumLines.push(`${sha256}  ${path.basename(p)}`);
  }
}

recordFile(targetPortable, 'dist/windows/ModelDeck-Portable.exe');
recordFile(targetVersionedPortable, `dist/windows/ModelDeck-${version}-Portable.exe`);
if (fs.existsSync(targetSetup)) {
  recordFile(targetSetup, 'dist/windows/ModelDeck-Setup.exe');
  recordFile(targetVersionedSetup, `dist/windows/ModelDeck-${version}-Setup.exe`);
}

if (fs.existsSync(optionalDir)) {
  for (const f of fs.readdirSync(optionalDir)) {
    recordFile(path.join(optionalDir, f), `dist/windows/optional/${f}`);
  }
}

const summary = {
  productName: 'ModelDeck',
  version,
  buildTime,
  gitCommit,
  artifacts,
};

fs.writeFileSync(
  path.join(distWindowsDir, 'release-metadata.json'),
  JSON.stringify(summary, null, 2),
  'utf-8'
);

fs.writeFileSync(
  path.join(distWindowsDir, 'checksums.sha256'),
  checksumLines.join('\n') + '\n',
  'utf-8'
);

console.log('\n=== Windows Release Artifacts Summary ===');
console.log(JSON.stringify(summary, null, 2));
console.log(`\nGenerated standard checksums at: ${path.join(distWindowsDir, 'checksums.sha256')}`);
