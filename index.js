const { existsSync, readFileSync } = require('fs');
const { join } = require('path');

const { platform, arch } = process;

let nativeBinding = null;

function loadNativeBinding(platformKey, archKey) {
  const fileName = 'index.node';
  const bindingPath = join(__dirname, 'native', `${platformKey}-${archKey}`, fileName);
  if (existsSync(bindingPath)) {
    nativeBinding = require(bindingPath);
  } else {
    throw new Error(`Unsupported platform: ${platformKey}-${archKey}`);
  }
}

switch (platform) {
  case 'linux':
    switch (arch) {
      case 'x64':
        loadNativeBinding('linux', 'x64');
        break;
      default:
        throw new Error(`Unsupported architecture on Linux: ${arch}`);
    }
    break;
  case 'darwin':
    switch (arch) {
      case 'x64':
        loadNativeBinding('darwin', 'x64');
        break;
      case 'arm64':
        loadNativeBinding('darwin', 'arm64');
        break;
      default:
        throw new Error(`Unsupported architecture on macOS: ${arch}`);
    }
    break;
  default:
    throw new Error(`Unsupported OS: ${platform}`);
}

module.exports = nativeBinding;