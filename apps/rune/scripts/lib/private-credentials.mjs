import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { chmod, mkdir, mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// The fixture always starts its own bus and keyring; it never connects to the caller's.
export async function startPrivateCredentials({ env: inherited = process.env } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'rune-private-credentials-'))
  await chmod(root, 0o700)
  const env = { ...inherited }
  for (const name of Object.keys(env)) {
    if (/^(DBUS_|GNOME_KEYRING_|SSH_AUTH_|GPG_AGENT_|XDG_SESSION_|DESKTOP_SESSION$)/.test(name)) delete env[name]
  }
  for (const [name, dir] of Object.entries({ HOME: 'home', XDG_CONFIG_HOME: 'config', XDG_DATA_HOME: 'data', XDG_CACHE_HOME: 'cache', XDG_RUNTIME_DIR: 'runtime' })) {
    env[name] = join(root, dir)
    await mkdir(env[name], { mode: 0o700 })
  }
  const control = join(root, 'control')
  await mkdir(control, { mode: 0o700 })
  const children = []
  let stopped = false
  const start = (command, args, options = {}) => {
    const child = spawn(command, args, { env, stdio: ['pipe', 'pipe', 'pipe'], ...options })
    // Always consume diagnostics, but never propagate them: service output can contain secrets.
    child.stdout?.resume(); child.stderr?.resume()
    child.on('error', () => {})
    child.on('close', () => { child.fixtureClosed = true })
    children.push(child)
    return child
  }
  const stop = async () => {
    if (stopped) return
    stopped = true
    for (const child of children.reverse()) {
      if (!child.fixtureClosed && child.exitCode === null && child.signalCode === null) {
        child.kill('SIGTERM')
        await new Promise(resolve => {
          const timer = setTimeout(() => { child.kill('SIGKILL') }, 2000)
          timer.unref()
          child.once('close', () => { clearTimeout(timer); resolve() })
        })
      }
    }
    // Preserve the private fixture for acceptance evidence.
  }
  const run = (command, args, input = '') => new Promise((resolve, reject) => {
    if (stopped) return reject(new Error('Private credential service is stopped'))
    const child = spawn(command, args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
    let out = ''
    child.stdout.setEncoding('utf8'); child.stdout.on('data', chunk => { out += chunk })
    child.stderr.resume()
    child.on('error', () => reject(new Error(`Private credential service could not run ${command}`)))
    child.on('close', code => code === 0 ? resolve(out) : reject(new Error(`Private credential service ${command} failed`)))
    child.stdin.on('error', () => {})
    child.stdin.end(input)
  })
  try {
    env.DBUS_SESSION_BUS_ADDRESS = `unix:path=${join(root, 'bus')}`
    // No activation directories: a failed private daemon must never launch a host service.
    const busConfig = join(root, 'bus.conf')
    await writeFile(busConfig, '<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>' + env.DBUS_SESSION_BUS_ADDRESS + '</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>', { mode: 0o600 })
    const bus = start('dbus-daemon', ['--nofork', `--config-file=${busConfig}`])
    const waitFor = async (probe, label) => {
      for (let i = 0; i < 100; i++) {
        if (stopped || bus.exitCode !== null || bus.signalCode !== null) throw new Error(`Private ${label} failed to start`)
        try { if (await probe()) return } catch {}
        await new Promise(resolve => setTimeout(resolve, 50))
      }
      throw new Error(`Private ${label} failed to start`)
    }
    const owner = name => run('gdbus', ['call', '--session', '--dest', 'org.freedesktop.DBus', '--object-path', '/org/freedesktop/DBus', '--method', 'org.freedesktop.DBus.NameHasOwner', name]).then(out => out.includes('true'))
    await waitFor(() => owner('org.freedesktop.DBus'), 'D-Bus')
    const daemon = start('gnome-keyring-daemon', ['--foreground', '--unlock', '--components=secrets', `--control-directory=${control}`])
    daemon.stdin.on('error', () => {})
    daemon.stdin.end(`${randomBytes(32).toString('hex')}\n`)
    await waitFor(() => owner('org.freedesktop.secrets'), 'Secret Service')
    async function putLegacyBundle(key, token, headers = {}) {
      if (typeof key !== 'string' || !/^legacy-auth-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(key)) throw new Error('Invalid Legacy credential reference')
      if ((token !== null && typeof token !== 'string') || !headers || typeof headers !== 'object' || Array.isArray(headers) || Object.values(headers).some(value => typeof value !== 'string')) throw new Error('Invalid synthetic Legacy credential bundle')
      await run('secret-tool', ['store', '--label=Rune private acceptance fixture', 'service', 'Hermes Rune', 'username', key, 'target', 'default', 'application', 'rust-keyring'], JSON.stringify({ version: 1, token, headers }))
    }
    return { env, putLegacyBundle, stop }
  } catch (error) {
    await stop()
    throw error
  }
}
