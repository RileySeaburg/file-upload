const { existsSync, readFileSync } = require('fs');
const { join } = require('path');

const { platform, arch } = process;

let nativeBinding = null;
let loadError = null;

function isMusl() {
  // For Node 10
  if (!process.report || typeof process.report.getReport !== 'function') {
    try {
      const lddPath = require('child_process').execSync('which ldd').toString().trim();
      return readFileSync(lddPath, 'utf8').includes('musl');
    } catch (e) {
      return true;
    }
  } else {
    const { glibcVersionRuntime } = process.report.getReport().header;
    return !glibcVersionRuntime;
  }
}

switch (platform) {
  case 'android':
    switch (arch) {
      case 'arm64':
        nativeBinding = require('./my-neon-project.android-arm64.node');
        break;
      case 'arm':
        nativeBinding = require('./my-neon-project.android-arm-eabi.node');
        break;
      default:
        throw new Error(`Unsupported architecture on Android ${arch}`);
    }
    break;
  case 'win32':
    switch (arch) {
      case 'x64':
        nativeBinding = require('./my-neon-project.win32-x64-msvc.node');
        break;
      case 'ia32':
        nativeBinding = require('./my-neon-project.win32-ia32-msvc.node');
        break;
      case 'arm64':
        nativeBinding = require('./my-neon-project.win32-arm64-msvc.node');
        break;
      default:
        throw new Error(`Unsupported architecture on Windows: ${arch}`);
    }
    break;
  case 'darwin':
    switch (arch) {
      case 'x64':
        nativeBinding = require('./my-neon-project.darwin-x64.node');
        break;
      case 'arm64':
        nativeBinding = require('./my-neon-project.darwin-arm64.node');
        break;
      default:
        throw new Error(`Unsupported architecture on macOS: ${arch}`);
    }
    break;
  case 'freebsd':
    if (arch !== 'x64') {
      throw new Error(`Unsupported architecture on FreeBSD: ${arch}`);
    }
    nativeBinding = require('./my-neon-project.freebsd-x64.node');
    break;
  case 'linux':
    switch (arch) {
      case 'x64':
        if (isMusl()) {
          nativeBinding = require('./my-neon-project.linux-x64-musl.node');
        } else {
          nativeBinding = require('./my-neon-project.linux-x64-gnu.node');
        }
        break;
      case 'arm64':
        if (isMusl()) {
          nativeBinding = require('./my-neon-project.linux-arm64-musl.node');
        } else {
          nativeBinding = require('./my-neon-project.linux-arm64-gnu.node');
        }
        break;
      case 'arm':
        nativeBinding = require('./my-neon-project.linux-arm-gnueabihf.node');
        break;
      default:
        throw new Error(`Unsupported architecture on Linux: ${arch}`);
    }
    break;
  default:
    throw new Error(`Unsupported OS: ${platform}, architecture: ${arch}`);
}

module.exports = nativeBinding;