import { spawn } from 'node:child_process'
import { startPrivateCredentials } from './lib/private-credentials.mjs'

const binary = process.argv[2]
if (!binary || process.argv.length !== 3) throw new Error('Pass the compiled Rune test executable')
const fixture = await startPrivateCredentials()
try {
  const child = spawn(binary, ['connections::core::legacy::tests::cloud_store::system_keyring_installs_rotates_and_retires_scoped_oauth_tokens', '--exact', '--ignored', '--nocapture'], { env: fixture.env, stdio: ['ignore', 'pipe', 'pipe'] })
  let output = ''
  child.stdout.on('data', chunk => { output += chunk })
  child.stderr.on('data', chunk => { output += chunk })
  const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve) })
  process.stdout.write(output)
  if (code !== 0 || !output.includes('1 passed; 0 failed')) throw new Error(`Private OAuth keyring test failed (${code})`)
} finally {
  await fixture.stop()
}
