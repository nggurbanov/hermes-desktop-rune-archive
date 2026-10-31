import { spawn } from 'node:child_process'
import { startPrivateCredentials } from './lib/private-credentials.mjs'

const binary = process.argv[2]
const selected = process.argv[3] || 'connections::core::legacy::ssh_store::tests::independent_process_lifetime_and_exact_generation_cleanup'
if (!binary || process.argv.length > 4) throw new Error('Pass the compiled Rune test executable and optional exact test name')
const fixture = await startPrivateCredentials()
try {
  const child = spawn(binary, [selected, '--exact', '--ignored', '--nocapture'], { env: fixture.env, stdio: ['ignore', 'pipe', 'pipe'], detached: true })
  let output = ''
  child.stdout.on('data', chunk => { output += chunk })
  child.stderr.on('data', chunk => { output += chunk })
  const timer = setTimeout(() => { try { process.kill(-child.pid, 'SIGKILL') } catch {} }, 30000)
  const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve) })
  clearTimeout(timer)
  process.stdout.write(output)
  if (code !== 0 || !output.includes('1 passed; 0 failed')) throw new Error(`Private SSH store test failed (${code})`)
} finally {
  await fixture.stop()
}
