const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const { checkVersions } = require('./check-version-consistency.cjs');

const root = path.resolve(__dirname, '..');
const portable = path.join(root, 'dist/windows/ModelDeck-Portable.exe');
const setup = path.join(root, 'dist/windows/ModelDeck-Setup.exe');
const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'ModelDeck-release-smoke-'));
const dataDir = path.join(sandbox, 'isolated-data');
const evidence = { startedAt: new Date().toISOString(), dataDir, checks: [], limitations: [
  'Process startup and database creation do not prove GUI rendering or real provider generation.',
  'Installer execution, offline startup, and ACL-denied directories are not tested.',
  'A read-only EXE attribute is not a read-only directory ACL.'
] };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function launch(exe, cwd, label) {
  const child = spawn(exe, [], { cwd, windowsHide: true, stdio: 'ignore',
    env: { ...process.env, MODELDECK_SMOKE_DATA_DIR: dataDir } });
  let exited = false;
  let spawnError;
  child.on('error', err => { spawnError = err; });
  child.on('exit', () => { exited = true; });
  try {
    await sleep(6000);
    if (spawnError) throw spawnError;
    assert(child.pid && !exited, `${label}: process exited early`);
    assert(fs.existsSync(path.join(dataDir, 'app.db')), `${label}: isolated database missing`);
    for (const name of ['images', 'videos', 'audio', 'files']) {
      assert(fs.statSync(path.join(dataDir, name)).isDirectory(), `${label}: ${name} missing`);
    }
    evidence.checks.push({ label, pid: child.pid, passed: true });
    console.log(`PASS: ${label}`);
  } finally {
    if (child.pid && !exited) {
      try { execFileSync('taskkill', ['/F', '/PID', String(child.pid), '/T'], { windowsHide: true, stdio: 'ignore' }); }
      catch { /* already exited */ }
    }
    await sleep(1000);
  }
}

async function main() {
  const versions = checkVersions();
  assert(versions.success, 'Version mismatch');
  evidence.version = versions.version;
  for (const file of [portable, setup]) {
    assert(fs.statSync(file).size > 64 * 1024, `Missing/invalid artifact: ${file}`);
    assert.equal(fs.readFileSync(file).subarray(0, 2).toString('ascii'), 'MZ', `Invalid PE executable: ${file}`);
  }
  const productVersion = execFileSync('powershell', ['-NoProfile', '-Command',
    '(Get-Item -LiteralPath $env:MODELDECK_VERIFY_EXE).VersionInfo.ProductVersion'], {
    env: { ...process.env, MODELDECK_VERIFY_EXE: portable }, windowsHide: true, encoding: 'utf8'
  }).trim();
  assert([versions.version, `${versions.version}.0`].includes(productVersion), `EXE version mismatch: ${productVersion}`);
  evidence.productVersion = productVersion;
  const chineseDir = path.join(sandbox, '中文 路径 带空格 (Portable 测试)');
  fs.mkdirSync(chineseDir);
  const exe = path.join(chineseDir, 'ModelDeck.exe');
  fs.copyFileSync(portable, exe);
  await launch(exe, os.tmpdir(), 'Chinese/spaced path and unrelated CWD');
  const movedDir = path.join(sandbox, '移动后 目录');
  fs.mkdirSync(movedDir);
  const moved = path.join(movedDir, 'ModelDeck.exe');
  fs.renameSync(exe, moved);
  await launch(moved, movedDir, 'Moved executable and existing isolated database');
  execFileSync('attrib', ['+R', moved], { windowsHide: true });
  try { await launch(moved, movedDir, 'Read-only executable attribute'); }
  finally { execFileSync('attrib', ['-R', moved], { windowsHide: true }); }
  evidence.completedAt = new Date().toISOString();
  evidence.passed = true;
}

main().catch(err => { evidence.passed = false; evidence.error = err.message; process.exitCode = 1; console.error(err); })
  .finally(() => {
    fs.writeFileSync(path.join(root, 'dist/windows/smoke-report.json'), JSON.stringify(evidence, null, 2) + '\n');
    console.log(`Evidence: ${path.join(root, 'dist/windows/smoke-report.json')}`);
    // Retain isolated data for inspection; never delete application/user data.
  });
