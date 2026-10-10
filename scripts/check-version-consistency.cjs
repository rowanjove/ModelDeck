const fs = require('fs');
const path = require('path');

function getCargoVersion(cargoTomlContent) {
  // 匹配 [package] 段落下的 version = "..."
  const packageSectionMatch = cargoTomlContent.match(/\[package\]([\s\S]*?)(?=\n\[|$)/);
  if (!packageSectionMatch) {
    throw new Error('Could not find [package] section in Cargo.toml');
  }
  const versionMatch = packageSectionMatch[1].match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!versionMatch) {
    throw new Error('Could not find version field under [package] in Cargo.toml');
  }
  return versionMatch[1];
}

function checkVersions(expectedVersion = null) {
  const rootDir = path.resolve(__dirname, '..');
  const pkgPath = path.join(rootDir, 'package.json');
  const cargoPath = path.join(rootDir, 'src-tauri', 'Cargo.toml');
  const tauriConfPath = path.join(rootDir, 'src-tauri', 'tauri.conf.json');

  if (!fs.existsSync(pkgPath)) throw new Error(`Missing ${pkgPath}`);
  if (!fs.existsSync(cargoPath)) throw new Error(`Missing ${cargoPath}`);
  if (!fs.existsSync(tauriConfPath)) throw new Error(`Missing ${tauriConfPath}`);

  const pkgJson = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  const pkgVersion = pkgJson.version;

  const cargoContent = fs.readFileSync(cargoPath, 'utf8');
  const cargoVersion = getCargoVersion(cargoContent);

  const tauriJson = JSON.parse(fs.readFileSync(tauriConfPath, 'utf8'));
  const tauriVersion = tauriJson.version;

  const versions = {
    'package.json': pkgVersion,
    'src-tauri/Cargo.toml': cargoVersion,
    'src-tauri/tauri.conf.json': tauriVersion,
  };

  console.log('[Version Check] Current configured versions:');
  for (const [file, ver] of Object.entries(versions)) {
    console.log(`  - ${file.padEnd(28)}: ${ver}`);
  }

  const isConsistent = pkgVersion === cargoVersion && cargoVersion === tauriVersion;
  if (!isConsistent) {
    console.error('\n[Error] Version mismatch detected across configuration files!');
    return { success: false, versions, expectedVersion };
  }

  if (expectedVersion) {
    const normalizedExpected = expectedVersion.startsWith('v') ? expectedVersion.slice(1) : expectedVersion;
    if (pkgVersion !== normalizedExpected) {
      console.error(`\n[Error] Configured version (${pkgVersion}) does not match expected version (${normalizedExpected})!`);
      return { success: false, versions, expectedVersion: normalizedExpected };
    }
  }

  console.log(`\n[Version Check] OK: All configuration files consistently specify version ${pkgVersion}.`);
  return { success: true, version: pkgVersion };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  let expected = null;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--expected' && args[i + 1]) {
      expected = args[i + 1];
      i++;
    } else if (!args[i].startsWith('-')) {
      expected = args[i];
    }
  }

  try {
    const result = checkVersions(expected);
    if (!result.success) {
      process.exit(1);
    }
  } catch (err) {
    console.error(`[Error] ${err.message}`);
    process.exit(1);
  }
}

module.exports = { checkVersions, getCargoVersion };
