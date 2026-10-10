'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { binaryPath } = require('../cli.cjs');

test('unsupported platforms fail with source-build guidance', () => {
  for (const [platform, arch] of [['freebsd', 'x64'], ['linux', 'ia32']]) {
    assert.throws(() => binaryPath(platform, arch), /Unsupported platform.*Build affine-local-mcp/);
  }
});

test('missing optional dependency gives actionable guidance', () => {
  const missing = () => {
    throw Object.assign(new Error('Not installed'), { code: 'MODULE_NOT_FOUND' });
  };
  assert.throws(() => binaryPath('darwin', 'x64', missing), /Missing native package.*optional dependencies/);
});

test('all six platforms resolve their matching executable', () => {
  for (const platform of ['darwin', 'linux', 'win32']) {
    for (const arch of ['x64', 'arm64']) {
      const executable = platform === 'win32' ? 'affine-local-mcp.exe' : 'affine-local-mcp';
      const resolve = (name) => {
        assert.equal(name, `affine-desktop-mcp-${platform}-${arch}/bin/${executable}`);
        return '/resolved/native/binary';
      };
      assert.equal(binaryPath(platform, arch, resolve), '/resolved/native/binary');
    }
  }
});

test('unexpected resolution errors retain their cause', () => {
  const error = Object.assign(new Error('Permission denied'), { code: 'EACCES' });
  assert.throws(() => binaryPath('linux', 'arm64', () => { throw error; }), (actual) => actual === error);
});
