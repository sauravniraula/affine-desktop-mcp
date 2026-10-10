#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');

function binaryPath(platform = process.platform, arch = process.arch, resolve = require.resolve) {
  if (!['darwin', 'linux', 'win32'].includes(platform) || !['x64', 'arm64'].includes(arch)) {
    throw new Error(`Unsupported platform ${platform}/${arch}. Build affine-local-mcp from source instead.`);
  }
  const name = `affine-desktop-mcp-${platform}-${arch}`;
  const executable = platform === 'win32' ? 'affine-local-mcp.exe' : 'affine-local-mcp';
  try {
    return resolve(`${name}/bin/${executable}`);
  } catch (error) {
    if (error.code !== 'MODULE_NOT_FOUND') throw error;
    throw new Error(`Missing native package ${name}. Reinstall affine-desktop-mcp with npm optional dependencies enabled (do not use --omit=optional).`);
  }
}

function main() {
  let binary;
  try {
    binary = binaryPath();
  } catch (error) {
    console.error(`affine-desktop-mcp: ${error.message}`);
    process.exitCode = 1;
    return;
  }
  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });
  // Forward signals sent to the wrapper alone; terminal process-group signals
  // may also reach the native process directly.
  const handlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM']) {
    const handler = () => child.kill(signal);
    handlers.set(signal, handler);
    process.on(signal, handler);
  }
  child.on('error', (error) => {
    console.error(`affine-desktop-mcp: ${error.message}`);
    process.exitCode = 1;
  });
  child.on('close', (code, signal) => {
    for (const [name, handler] of handlers) process.removeListener(name, handler);
    if (signal && process.platform !== 'win32') {
      process.kill(process.pid, signal);
    } else {
      process.exitCode = code ?? 1;
    }
  });
}

if (require.main === module) main();
module.exports = { binaryPath };
