import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { createHash, randomBytes, randomUUID } from 'node:crypto'
import { readdir, readFile, stat } from 'node:fs/promises'
import { dirname } from 'node:path'
import { promisify } from 'node:util'
import test from 'node:test'
import { startPrivateCredentials } from './private-credentials.mjs'

const exec = promisify(execFile)

test('private Secret Service stores exact Rune attributes, isolates the inherited environment and retains its root while stopping owned services', async () => {
  const fakeBus = 'unix:path=/does-not-exist/user-session'
  const fixture = await startPrivateCredentials({ env: { ...process.env, DBUS_SESSION_BUS_ADDRESS: fakeBus, GNOME_KEYRING_CONTROL: '/must-not-use', SSH_AUTH_SOCK: '/must-not-use', DISPLAY: ':fixture-display' } })
  const root = dirname(fixture.env.HOME)
  const key = `legacy-auth-${randomUUID()}`
  const ownerPid = async name => {
    const { stdout } = await exec('gdbus', ['call', '--session', '--dest', 'org.freedesktop.DBus', '--object-path', '/org/freedesktop/DBus', '--method', 'org.freedesktop.DBus.GetConnectionUnixProcessID', name], { env: fixture.env })
    const pid = Number(stdout.match(/uint32 (\d+)/)?.[1]); assert.ok(pid > 1); return pid
  }
  const owned = await Promise.all(['org.freedesktop.DBus', 'org.freedesktop.secrets'].map(ownerPid))
  const identities = await Promise.all(owned.map(async pid => ({ pid, start: (await readFile(`/proc/${pid}/stat`, 'utf8')).split(') ')[1].split(' ')[19] })))
  const token = randomBytes(32).toString('hex')
  const header = randomBytes(32).toString('hex')
  try {
    assert.notEqual(fixture.env.DBUS_SESSION_BUS_ADDRESS, fakeBus)
    assert.equal(fixture.env.GNOME_KEYRING_CONTROL, undefined)
    assert.equal(fixture.env.SSH_AUTH_SOCK, undefined)
    assert.equal(fixture.env.DISPLAY, ':fixture-display')
    await fixture.putLegacyBundle(key, token, { 'X-Fixture': header })
    const { stdout } = await exec('secret-tool', ['lookup', 'service', 'Hermes Rune', 'username', key, 'target', 'default'], { env: fixture.env })
    const digest = value => createHash('sha256').update(value).digest('hex')
    assert.equal(digest(stdout.trim()), digest(JSON.stringify({ version: 1, token, headers: { 'X-Fixture': header } })))
    await assert.rejects(exec('secret-tool', ['lookup', 'service', 'Hermes Rune', 'username', key, 'target', 'wrong'], { env: fixture.env }))
    async function checkFiles(dir) {
      for (const entry of await readdir(dir, { withFileTypes: true })) {
        const path = `${dir}/${entry.name}`
        if (entry.isDirectory()) await checkFiles(path)
        else if (entry.isFile()) {
          const contents = await readFile(path)
          assert.equal(contents.includes(token), false)
          assert.equal(contents.includes(header), false)
        }
      }
    }
    await checkFiles(root)
  } finally {
    await fixture.stop()
  }
  assert.ok((await stat(root)).isDirectory())
  for (const { pid, start } of identities) {
    try {
      const current = (await readFile(`/proc/${pid}/stat`, 'utf8')).split(') ')[1].split(' ')[19]
      assert.notEqual(current, start, `owned process ${pid} still running`)
    } catch (error) { if (error.code !== 'ENOENT' && error.code !== 'ESRCH') throw error }
  }
  await assert.rejects(fixture.putLegacyBundle(key, token, {}), /stopped/)
  await assert.rejects(exec('secret-tool', ['lookup', 'service', 'Hermes Rune', 'username', key], { env: fixture.env }))
  await fixture.stop()
})

test('a failed private service is visible and does not activate a replacement', async () => {
  const fixture = await startPrivateCredentials()
  try {
    const args = ['call', '--session', '--dest', 'org.freedesktop.DBus', '--object-path', '/org/freedesktop/DBus', '--method', 'org.freedesktop.DBus.GetConnectionUnixProcessID', 'org.freedesktop.secrets']
    const { stdout } = await exec('gdbus', args, { env: fixture.env })
    const pid = Number(stdout.match(/uint32 (\d+)/)?.[1])
    assert.ok(pid > 1)
    process.kill(pid, 'SIGTERM')
    const ownerArgs = ['call', '--session', '--dest', 'org.freedesktop.DBus', '--object-path', '/org/freedesktop/DBus', '--method', 'org.freedesktop.DBus.NameHasOwner', 'org.freedesktop.secrets']
    for (let i = 0; i < 100; i++) {
      const owner = await exec('gdbus', ownerArgs, { env: fixture.env })
      if (owner.stdout.includes('false')) break
      if (i === 99) throw new Error('Private service did not stop')
      await new Promise(resolve => setTimeout(resolve, 20))
    }
    await assert.rejects(fixture.putLegacyBundle(`legacy-auth-${randomUUID()}`, randomBytes(32).toString('hex'), {}), /failed/)
    const owner = await exec('gdbus', ownerArgs, { env: fixture.env })
    assert.match(owner.stdout, /false/)
  } finally {
    await fixture.stop()
  }
})
