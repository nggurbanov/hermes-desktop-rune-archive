#!/usr/bin/env node
// Bind packaging to the frozen build inputs, never to the mutable checkout.
import { createHash } from 'node:crypto'
import { chmodSync, constants, copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { appDir } from './package-linux.mjs'
import { options, required, run } from './package-common.mjs'

export function sha(path) { return createHash('sha256').update(readFileSync(path)).digest('hex') }
function verify(path, expected, label) {
  if (!/^[a-f0-9]{64}$/.test(expected || '') || sha(path) !== expected) throw new Error(`${label} SHA-256 mismatch`)
}
export function packageFrozen(o) {
  const manifestPath = resolve(required(o, 'manifest'))
  const expectedManifest = required(o, 'manifest-sha256')
  const temp = mkdtempSync(join(tmpdir(), 'rune provenance '))
  try {
    const manifestCopy = join(temp, 'source-manifest.json')
    copyFileSync(manifestPath, manifestCopy)
    verify(manifestCopy, expectedManifest, 'source manifest')
    const manifest = JSON.parse(readFileSync(manifestCopy, 'utf8'))
    const listSha = manifest.source_list_sha256 || manifest.file_list_sha256
    if (manifest.source_list_sha256 && manifest.file_list_sha256 && manifest.source_list_sha256 !== manifest.file_list_sha256) throw new Error('Conflicting source file list hashes')
    const binary = join(temp, 'hermes-rune')
    const archive = join(temp, 'source.tar')
    const list = join(temp, 'source-files.list0')
    copyFileSync(resolve(required(o, 'binary')), binary)
    copyFileSync(resolve(required(o, 'source-archive')), archive)
    copyFileSync(resolve(required(o, 'source-list')), list)
    verify(binary, manifest.candidate_sha256, 'binary')
    verify(archive, manifest.source_archive_sha256, 'source archive')
    verify(list, listSha, 'source file list')
    if (o.appimage) {
      if (existsSync(resolve(o.appimage))) throw new Error('AppImage output already exists')
      copyFileSync(resolve(required(o, 'appimagetool')), join(temp, 'appimagetool'))
      copyFileSync(resolve(required(o, 'runtime')), join(temp, 'runtime'))
      verify(join(temp, 'appimagetool'), required(o, 'appimagetool-sha256'), 'appimagetool')
      verify(join(temp, 'runtime'), required(o, 'runtime-sha256'), 'AppImage runtime')
      chmodSync(join(temp, 'appimagetool'), 0o700)
    }
    const source = join(temp, 'source')
    run('mkdir', ['--', source])
    run('tar', ['-xf', archive, '-C', source, '--no-same-owner'])
    appDir({...o, binary, sourceRoot: source})
    const doc = join(resolve(o.out), 'usr/share/doc/hermes-desktop-rune')
    verify(join(resolve(o.out), 'usr/bin/hermes-desktop-rune'), manifest.candidate_sha256, 'packaged binary')
    copyFileSync(manifestCopy, join(doc, 'source-manifest.json'))
    copyFileSync(list, join(doc, 'source-files.list0'))
    writeFileSync(join(doc, 'PROVENANCE.json'), JSON.stringify({schema: 1, source_manifest_sha256: expectedManifest, binary_sha256: manifest.candidate_sha256, source_archive_sha256: manifest.source_archive_sha256, source_file_list_sha256: listSha, git_head: manifest.git_head}, null, 2) + '\n')
    // Verify the original inputs again, so concurrent replacement cannot go unnoticed.
    verify(manifestPath, expectedManifest, 'source manifest after packaging')
    verify(resolve(o.binary), manifest.candidate_sha256, 'binary after packaging')
    verify(resolve(o['source-archive']), manifest.source_archive_sha256, 'source archive after packaging')
    verify(resolve(o['source-list']), listSha, 'source list after packaging')
    const sourceOutput = resolve(required(o, 'source-output'))
    copyFileSync(archive, sourceOutput, constants.COPYFILE_EXCL)
    verify(sourceOutput, manifest.source_archive_sha256, 'copied source archive')
    if (o.appimage) {
      run(join(temp, 'appimagetool'), ['--runtime-file', join(temp, 'runtime'), resolve(o.out), resolve(o.appimage)])
      verify(resolve(o.appimagetool), o['appimagetool-sha256'], 'appimagetool after packaging')
      verify(resolve(o.runtime), o['runtime-sha256'], 'AppImage runtime after packaging')
    }
  } finally { rmSync(temp, {recursive: true, force: true}) }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) packageFrozen(options(process.argv.slice(2)))
