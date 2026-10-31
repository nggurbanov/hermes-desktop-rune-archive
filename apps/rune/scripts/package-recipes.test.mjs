import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { appDir, main as linuxMain } from './package-linux.mjs'
import { appBundle } from './package-macos.mjs'
import { packageTree } from './package-windows.mjs'

test('packages a fixture in a spaced path and preserves child arguments', () => {
  const temp = mkdtempSync(join(tmpdir(), 'rune package ü '))
  try {
    const binary = join(temp, 'fixture')
    writeFileSync(binary, '#!/bin/sh\nprintf "%s\\n" "$@"\n')
    if (process.platform !== 'win32') {
      const linux = appDir({binary, out: join(temp, 'App Dir'), version: '1.2.3'})
      const child = spawnSync(join(linux, 'AppRun'), ['--child', 'a b'], {encoding:'utf8'})
      assert.equal(child.status, 0); assert.equal(child.stdout, '--child\na b\n')
      assert.match(readFileSync(join(linux, 'io.github.nggurbanov.hermes-desktop-rune.desktop'), 'utf8'), /x-scheme-handler\/hermes-rune/)
      assert.throws(() => appDir({binary, out:linux, version:'1.2.3'}), /already exists/)
      assert.match(readFileSync(join(linux, 'usr/share/doc/hermes-desktop-rune/modified-source/symphonia-core/LICENSE-MPL-2.0.txt'), 'utf8'), /Mozilla Public License/)
      assert.throws(() => linuxMain({binary, out:join(temp, 'no-runtime'), version:'1.2.3', appimage:join(temp, 'out.AppImage')}), /Missing --runtime/)
    }
    const mac = appBundle({binary, out: join(temp, 'Hermes.app'), version:'1.2.3'})
    assert.match(readFileSync(join(mac, 'Contents/Info.plist'), 'utf8'), /NSMicrophoneUsageDescription/)
    const win = packageTree({binary, out:join(temp, 'MSIX'), version:'1.2.3', publisher:'CN=Test & Co', arch:'x64'})
    const manifest = readFileSync(join(win, 'AppxManifest.xml'), 'utf8')
    assert.match(manifest, /CN=Test &amp; Co/); assert.match(manifest, /Version="1.2.3.0"/)
    assert.throws(() => packageTree({binary, out:join(temp,'bad'), version:'1.2.3-beta', publisher:'CN=Test', arch:'x64'}), /Version/)
  } finally { rmSync(temp, {recursive:true, force:true}) }
})
