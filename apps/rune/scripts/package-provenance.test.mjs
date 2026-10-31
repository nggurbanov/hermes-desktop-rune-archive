import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, dirname } from 'node:path'
import { spawnSync } from 'node:child_process'
import { packageFrozen, sha } from './package-provenance.mjs'

test('frozen package binds source and binary and rejects mismatches in Unicode paths', () => {
  const t = mkdtempSync(join(tmpdir(), 'rune provenance ü '))
  try {
    const root = join(t, 'snapshot')
    const put = (p, data) => { const dest = join(root, p); mkdirSync(dirname(dest), {recursive:true}); writeFileSync(dest, data) }
    for (const p of ['LICENSE', 'apps/desktop/assets/icon.png', 'apps/rune/THIRD_PARTY_NOTICES.md', 'apps/rune/licenses/license', 'apps/rune/assets/plugin-host/THIRD_PARTY_LICENSES.txt', ...['symphonia-core','symphonia-metadata','symphonia-format-riff','symphonia-format-isomp4'].map(c => `apps/rune/vendor/${c}/license`)]) put(p, 'frozen')
    const archive = join(t, 'source ü.tar'), list = join(t, 'list'), binary = join(t, 'binary'), manifest = join(t, 'manifest')
    assert.equal(spawnSync('tar', ['-cf', archive, '-C', root, '.']).status, 0)
    writeFileSync(binary, 'frozen binary'); writeFileSync(list, 'fixture\0')
    writeFileSync(manifest, JSON.stringify({candidate_sha256:sha(binary), source_archive_sha256:sha(archive), source_list_sha256:sha(list), git_head:'fixture'}))
    const opts = {binary, manifest, 'manifest-sha256':sha(manifest), 'source-archive':archive, 'source-list':list, 'source-output':join(t,'copied source.tar'), out:join(t,'App Dir'), version:'1.2.3'}
    packageFrozen(opts)
    const doc = join(opts.out, 'usr/share/doc/hermes-desktop-rune')
    assert.equal(readFileSync(join(doc,'LICENSE'),'utf8'), 'frozen')
    assert.equal(JSON.parse(readFileSync(join(doc,'PROVENANCE.json'))).binary_sha256, sha(binary))
    assert.equal(sha(opts['source-output']), sha(archive))
    const conflicting = join(t, 'conflicting manifest')
    writeFileSync(conflicting, JSON.stringify({candidate_sha256:sha(binary), source_archive_sha256:sha(archive), source_list_sha256:sha(list), file_list_sha256:'0'.repeat(64)}))
    assert.throws(() => packageFrozen({...opts, manifest:conflicting, 'manifest-sha256':sha(conflicting), out:join(t, 'conflicting out')}), /Conflicting source file list hashes/)
    for (const key of ['binary', 'source-archive', 'source-list', 'manifest']) {
      const bad = join(t, `bad ${key}`); writeFileSync(bad, 'tampered')
      assert.throws(() => packageFrozen({...opts, [key]:bad, out:join(t, `bad out ${key}`)}), /mismatch/)
    }
    assert.throws(() => packageFrozen({...opts, 'manifest-sha256':'0'.repeat(64), out:join(t,'bad digest')}), /mismatch/)
    const tool = join(t, 'appimagetool'), runtime = join(t, 'runtime')
    writeFileSync(tool, 'tool'); writeFileSync(runtime, 'runtime')
    const imageOpts = {...opts, out:join(t, 'bad tool out'), appimage:join(t, 'image'), appimagetool:tool, runtime, 'appimagetool-sha256':'0'.repeat(64), 'runtime-sha256':sha(runtime)}
    assert.throws(() => packageFrozen(imageOpts), /appimagetool SHA-256 mismatch/)
    assert.throws(() => packageFrozen({...imageOpts, 'appimagetool-sha256':sha(tool), 'runtime-sha256':'0'.repeat(64)}), /runtime SHA-256 mismatch/)
  } finally { rmSync(t, {recursive:true, force:true}) }
})
