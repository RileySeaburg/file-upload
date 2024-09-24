const { existsSync, readFileSync } = require('fs');
const { join } = require('path');

const { platform, arch } = process;

let nativeBinding = null;

function loadNativeBinding(platformKey) {
  const fileName = `file-upload.${platformKey}.node`;
  const bindingPath = join(__dirname, fileName);
  if (existsSync(bindingPath)) {
    nativeBinding = require(bindingPath);
  } else {
    throw new Error(`Unsupported platform: ${platformKey}`);
  }
}

switch (platform) {
  case 'linux':
    loadNativeBinding('linux-x64-gnu');
    break;
  case 'darwin':
    loadNativeBinding('darwin-x64');
    break;
  case 'win32':
    loadNativeBinding('win32-x64-msvc');
    break;
  default:
    throw new Error(`Unsupported OS: ${platform}`);
}

module.exports = nativeBinding;