const fs = require('fs');
const path = require('path');

const target = process.argv[2];
if (!target) {
  console.error("Please specify target platform: mac-arm, mac-intel, or win");
  process.exit(1);
}

const versionInfo = JSON.parse(fs.readFileSync(path.join(__dirname, '../version.json'), 'utf8'));
const version = versionInfo.version;
const appName = 'auDO-File-Z';

const releaseDir = path.join(__dirname, '../releases');
if (!fs.existsSync(releaseDir)) {
  fs.mkdirSync(releaseDir);
}

const targetDir = path.join(__dirname, '../src-tauri/target');

// Helper to find a file in a folder that matches a suffix or extension
const findFile = (folder, suffix) => {
  if (!fs.existsSync(folder)) return null;
  const files = fs.readdirSync(folder);
  const match = files.find(f => f.toLowerCase().includes(suffix.toLowerCase()));
  return match ? path.join(folder, match) : null;
};

if (target === 'mac-arm') {
  const folder = path.join(targetDir, 'aarch64-apple-darwin/release/bundle/dmg');
  const src = findFile(folder, '.dmg');
  const dest = path.join(releaseDir, `${appName}-v${version}-macos-arm64.dmg`);
  if (src && fs.existsSync(src)) {
    fs.copyFileSync(src, dest);
    console.log(`Successfully packaged Apple Silicon DMG: ${dest}`);
  } else {
    console.error(`Build artifact not found in: ${folder}`);
    process.exit(1);
  }
} else if (target === 'mac-intel') {
  const folder = path.join(targetDir, 'x86_64-apple-darwin/release/bundle/dmg');
  const src = findFile(folder, '.dmg');
  const dest = path.join(releaseDir, `${appName}-v${version}-macos-intel.dmg`);
  if (src && fs.existsSync(src)) {
    fs.copyFileSync(src, dest);
    console.log(`Successfully packaged Intel Mac DMG: ${dest}`);
  } else {
    console.error(`Build artifact not found in: ${folder}`);
    process.exit(1);
  }
} else if (target === 'win') {
  const folderNormal = path.join(targetDir, 'release/bundle/nsis');
  const folderTriple = path.join(targetDir, 'x86_64-pc-windows-msvc/release/bundle/nsis');
  const folder = fs.existsSync(folderTriple) ? folderTriple : folderNormal;
  const src = findFile(folder, '.exe');
  const dest = path.join(releaseDir, `${appName}-v${version}-windows-x64.exe`);
  if (src && fs.existsSync(src)) {
    fs.copyFileSync(src, dest);
    console.log(`Successfully packaged Windows x64 Executable: ${dest}`);
  } else {
    console.error(`Build artifact not found in: ${folder}`);
    process.exit(1);
  }
}
