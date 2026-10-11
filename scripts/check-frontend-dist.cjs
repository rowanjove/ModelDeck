const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const frontend = path.resolve(root, 'src-tauri', config.build.frontendDist);
if (frontend !== path.join(root, 'dist/frontend')) {
  throw new Error('Frontend assets must be isolated in dist/frontend, separate from dist/windows.');
}
let bytes = 0;
let count = 0;
function visit(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Symlink not allowed in frontend assets: ${file}`);
    if (entry.isDirectory()) {
      visit(file);
    } else {
      if (/\.(exe|msi|zip|dll|pdb)$/i.test(entry.name)) {
        throw new Error(`Packaged binary must not be embedded in frontend assets: ${file}`);
      }
      bytes += fs.statSync(file).size;
      count++;
    }
  }
}
visit(frontend);
if (bytes > 10 * 1024 * 1024) throw new Error(`Frontend assets unexpectedly exceed 10 MiB: ${bytes} bytes`);
console.log(`Frontend resource boundary passed: ${count} files, ${bytes} bytes.`);
