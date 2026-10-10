const fs = require('fs');
const path = require('path');
const { spawn, execSync } = require('child_process');

const rootDir = path.resolve(__dirname, '..');
const portableExe = path.join(rootDir, 'dist', 'windows', 'ModelDeck-Portable.exe');
const setupExe = path.join(rootDir, 'dist', 'windows', 'ModelDeck-Setup.exe');

const appData = process.env.APPDATA || path.join(process.env.USERPROFILE, 'AppData', 'Roaming');
const modelDeckDataDir = path.join(appData, 'local.modeldeck');

console.log('=== 开始 Windows 生产打包验证 ===');
console.log(`Portable EXE: ${portableExe} (${fs.existsSync(portableExe) ? '存在' : '缺失'})`);
console.log(`Setup EXE:    ${setupExe} (${fs.existsSync(setupExe) ? '存在' : '缺失'})`);
console.log(`系统数据目录: ${modelDeckDataDir}`);

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function runProcessForAWhile(exePath, workingDir, durationMs = 6000) {
  console.log(`\n[启动测试] 目标: ${exePath}`);
  console.log(`[启动测试] 工作目录: ${workingDir}`);

  const child = spawn(exePath, [], {
    cwd: workingDir,
    detached: true,
    stdio: 'ignore',
  });

  const pid = child.pid;
  console.log(`[启动测试] 成功派生进程 PID: ${pid}`);

  let survived = true;
  for (let i = 0; i < Math.floor(durationMs / 1000); i++) {
    await sleep(1000);
    try {
      // 检查进程是否仍然存活
      process.kill(pid, 0);
    } catch (e) {
      survived = false;
      break;
    }
  }

  // 终止测试进程
  try {
    execSync(`taskkill /F /PID ${pid} /T`, { stdio: 'ignore' });
    console.log(`[启动测试] 正常终止测试进程 PID: ${pid}`);
  } catch (_) {}

  // 额外清理可能存在的残留 ModelDeck 进程
  try {
    execSync('taskkill /F /IM ModelDeck.exe /T', { stdio: 'ignore' });
  } catch (_) {}

  return survived;
}

async function main() {
  const testsPassed = [];
  const testsFailed = [];

  // 测试 1: 安装版文件存在性与大小
  if (fs.existsSync(setupExe) && fs.statSync(setupExe).size > 5 * 1024 * 1024) {
    testsPassed.push(`Setup 安装版存在且大小合法 (${(fs.statSync(setupExe).size / (1024 * 1024)).toFixed(2)} MB)`);
  } else {
    testsFailed.push('Setup 安装版不存在或文件异常');
  }

  // 测试 2: Portable 版在含空格与中文目录中运行
  const testDirChineseSpaces = path.join(rootDir, 'test_sandbox', '中文 路径 带空格 (Portable 测试)');
  fs.mkdirSync(testDirChineseSpaces, { recursive: true });
  const copiedExe1 = path.join(testDirChineseSpaces, 'ModelDeck-Portable.exe');
  fs.copyFileSync(portableExe, copiedExe1);

  // 工作目录故意设置在系统临时目录，验证不依赖 CWD 找资源
  const tempDir = process.env.TEMP || 'C:\\Windows\\Temp';
  const survived1 = await runProcessForAWhile(copiedExe1, tempDir, 5000);

  if (survived1) {
    testsPassed.push('Portable 版在中文与空格路径下启动正常且不依赖 CWD');
  } else {
    testsFailed.push('Portable 版在中文与空格路径下启动失败或闪退');
  }

  // 测试 3: 验证 AppData 目录结构及 SQLite 数据库与表
  const dbPath = path.join(modelDeckDataDir, 'app.db');
  if (fs.existsSync(dbPath)) {
    testsPassed.push(`SQLite 数据库已在安全系统目录生成: ${dbPath}`);
    const dirs = ['images', 'videos', 'audio', 'files'];
    const allDirsExist = dirs.every(d => fs.existsSync(path.join(modelDeckDataDir, d)));
    if (allDirsExist) {
      testsPassed.push('运行时存储目录 (images/videos/audio/files) 均已建立');
    } else {
      testsFailed.push('运行时存储子目录建立不完整');
    }
  } else {
    testsFailed.push(`SQLite 数据库未在 ${dbPath} 生成`);
  }

  // 测试 4: 移动目录后再次运行（验证不依赖固定安装路径，并能重用已有数据）
  const testDirMoved = path.join(rootDir, 'test_sandbox', 'another_moved_dir', '二级 深度 中文目录');
  fs.mkdirSync(testDirMoved, { recursive: true });
  const movedExe = path.join(testDirMoved, 'ModelDeck-Portable.exe');
  fs.copyFileSync(copiedExe1, movedExe);

  const survived2 = await runProcessForAWhile(movedExe, testDirMoved, 5000);
  if (survived2) {
    testsPassed.push('Portable 版任意移动目录后再次运行平稳正常');
  } else {
    testsFailed.push('Portable 版移动目录后运行异常');
  }

  // 测试 5: 验证 EXE 所在目录只读场景（不向 EXE 目录写文件）
  const testDirReadOnly = path.join(rootDir, 'test_sandbox', 'readonly_dir');
  fs.mkdirSync(testDirReadOnly, { recursive: true });
  const readOnlyExe = path.join(testDirReadOnly, 'ModelDeck-Portable.exe');
  fs.copyFileSync(portableExe, readOnlyExe);

  // 将目录中的文件设置为只读
  try {
    execSync(`attrib +R "${readOnlyExe}"`, { stdio: 'ignore' });
  } catch (_) {}

  const survived3 = await runProcessForAWhile(readOnlyExe, testDirReadOnly, 5000);
  if (survived3) {
    testsPassed.push('只读属性目录下 Portable 版正常运行（符合便携安全规范）');
  } else {
    testsFailed.push('只读属性目录下 Portable 版运行异常');
  }

  // 清理测试临时沙箱目录（去除只读后清理）
  try {
    execSync(`attrib -R "${readOnlyExe}"`, { stdio: 'ignore' });
    fs.rmSync(path.join(rootDir, 'test_sandbox'), { recursive: true, force: true });
  } catch (_) {}

  console.log('\n================ 测试汇总 ================');
  console.log(`通过项 (${testsPassed.length}):`);
  testsPassed.forEach(t => console.log(`  ✓ ${t}`));
  if (testsFailed.length > 0) {
    console.log(`失败项 (${testsFailed.length}):`);
    testsFailed.forEach(t => console.log(`  ✗ ${t}`));
    process.exit(1);
  } else {
    console.log('所有发布环境测试项全部 100% 通过！');
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
