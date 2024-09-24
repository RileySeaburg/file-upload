const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const currentPlatform = os.platform();
const currentArch = os.arch();

const targets = [
  { platform: 'linux', arch: 'x64', target: 'x86_64-unknown-linux-gnu', command: 'npm run build:linux', binaryName: 'libfile_upload.so' },
  { platform: 'darwin', arch: 'x64', target: 'x86_64-apple-darwin', command: 'npm run build:macos', binaryName: 'libfile_upload.dylib' },
  { platform: 'darwin', arch: 'arm64', target: 'aarch64-apple-darwin', command: 'npm run build:macos', binaryName: 'libfile_upload.dylib' }
];

const buildDir = path.join(__dirname, 'native');
if (!fs.existsSync(buildDir)) {
  fs.mkdirSync(buildDir, { recursive: true });
}

function buildForTarget(target) {
  console.log(`Building for ${target.platform}-${target.arch}...`);

  try {
    console.log(`Running build command: ${target.command}`);
    execSync(target.command, { stdio: 'inherit' });
    
    const sourcePath = path.join(__dirname, 'target', target.target, 'release', target.binaryName);
    const destPath = path.join(buildDir, `${target.platform}-${target.arch}`, 'index.node');
    
    if (!fs.existsSync(sourcePath)) {
      throw new Error(`Could not find compiled binary at ${sourcePath}`);
    }

    fs.mkdirSync(path.dirname(destPath), { recursive: true });
    fs.copyFileSync(sourcePath, destPath);

    console.log(`Binary built successfully for ${target.platform}-${target.arch}`);
    console.log(`Copied from ${sourcePath} to ${destPath}`);
  } catch (error) {
    console.error(`Failed to build for ${target.platform}-${target.arch}:`, error);
  }
}

// Build for all targets
targets.forEach(buildForTarget);

console.log('Build process completed for all targets.');