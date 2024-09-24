const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const currentPlatform = os.platform();
const currentArch = os.arch();

const targets = [
  { platform: 'linux', arch: 'x64', target: 'x86_64-unknown-linux-musl' },
  { platform: 'darwin', arch: 'x64', target: 'x86_64-apple-darwin' },
  { platform: 'darwin', arch: 'arm64', target: 'aarch64-apple-darwin' },
];

const buildDir = path.join(__dirname, 'native');
if (!fs.existsSync(buildDir)) {
  fs.mkdirSync(buildDir, { recursive: true });
}

function buildForTarget(target) {
  console.log(`Building for ${target.platform}-${target.arch}...`);

  try {
    let buildCommand;
    let env = { ...process.env };

    if (target.platform === 'linux') {
      buildCommand = 'npm run build:linux';
      // Set environment variables for Linux build
      env.CC_x86_64_unknown_linux_musl = 'x86_64-linux-musl-gcc';
      env.CXX_x86_64_unknown_linux_musl = 'x86_64-linux-musl-g++';
      env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = 'x86_64-linux-musl-gcc';
      // Add the musl-cross bin directory to PATH
      env.PATH = `/opt/homebrew/opt/musl-cross/bin:${env.PATH}`;
    } else {
      buildCommand = 'npm run build';
    }
    
    console.log(`Running build command: ${buildCommand}`);
    execSync(buildCommand, { stdio: 'inherit', env });
    
    const sourcePath = path.join(__dirname, 'index.node');
    const destPath = path.join(buildDir, `${target.platform}-${target.arch}`, 'index.node');
    
    if (!fs.existsSync(sourcePath)) {
      throw new Error(`Could not find compiled binary at ${sourcePath}`);
    }

    fs.mkdirSync(path.dirname(destPath), { recursive: true });
    fs.copyFileSync(sourcePath, destPath);

    console.log(`Binary built successfully for ${target.platform}-${target.arch}`);
    console.log(`Copied from ${sourcePath} to ${destPath}`);

    // Clean up the source binary
    fs.unlinkSync(sourcePath);
  } catch (error) {
    console.error(`Failed to build for ${target.platform}-${target.arch}:`, error);
  }
}

// Build for all targets
targets.forEach(buildForTarget);

console.log('Build process completed for all targets.');