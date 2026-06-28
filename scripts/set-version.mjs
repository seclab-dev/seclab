import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, '..');

const version = process.argv[2];
if (!version) {
  console.error('Error: Please specify a version. Example: pnpm version:set 0.1.1');
  process.exit(1);
}

const semverRegex = /^\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?$/;
if (!semverRegex.test(version)) {
  console.error(`Error: Invalid SemVer format '${version}'`);
  process.exit(1);
}

// 1. Update frontend/package.json
const pkgPath = path.resolve(rootDir, 'frontend/package.json');
if (fs.existsSync(pkgPath)) {
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  pkg.version = version;
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8');
  console.log(`Updated frontend/package.json version to ${version}`);
} else {
  console.warn('Warning: frontend/package.json not found');
}

// 2. Recursively find and update Cargo.toml files
function findCargoTomls(dir, fileList = []) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    const filePath = path.join(dir, file);
    const stat = fs.statSync(filePath);
    if (stat.isDirectory()) {
      if (file !== 'target' && file !== '.git' && file !== 'node_modules' && file !== 'frontend') {
        findCargoTomls(filePath, fileList);
      }
    } else if (file === 'Cargo.toml') {
      fileList.push(filePath);
    }
  }
  return fileList;
}

function updateCargoToml(filePath, newVersion) {
  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split('\n');
  let inPackageBlock = false;
  let versionUpdated = false;
  
  const updatedLines = lines.map(line => {
    const trimmed = line.trim();
    if (trimmed.startsWith('[package]') || trimmed.startsWith('[workspace.package]')) {
      inPackageBlock = true;
      return line;
    }
    if (trimmed.startsWith('[')) {
      inPackageBlock = false;
      return line;
    }
    if (inPackageBlock && !versionUpdated) {
      const match = line.match(/^version\s*=\s*"[^"]+"/);
      if (match) {
        versionUpdated = true;
        return line.replace(/version\s*=\s*"[^"]+"/, `version = "${newVersion}"`);
      }
    }
    return line;
  });
  
  if (versionUpdated) {
    fs.writeFileSync(filePath, updatedLines.join('\n'), 'utf8');
    console.log(`Updated ${path.relative(rootDir, filePath)} version to ${newVersion}`);
  }
}

const cargoTomls = findCargoTomls(rootDir);
for (const toml of cargoTomls) {
  updateCargoToml(toml, version);
}
console.log('Version synchronization complete.');
