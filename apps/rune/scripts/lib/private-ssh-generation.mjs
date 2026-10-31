import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

// Capture identity while the owned generation is live. Never identify a process by argv.
export function captureGeneration(fixtureHome, owner) {
  assert.match(owner, /^[0-9a-f]{32}$/)
  const directory = path.join(fixtureHome, '.hermes', 'desktop-ssh', owner)
  const record = JSON.parse(fs.readFileSync(path.join(directory, 'backend.lock.json'), 'utf8'))
  assert.equal(record.ownershipId, owner)
  assert.match(record.spawnNonce, /^[0-9a-f]{16}$/)
  assert.ok(Number.isSafeInteger(record.pid) && record.pid > 0)
  assert.ok(Number.isSafeInteger(record.startTicks) && record.startTicks >= 0)
  return Object.freeze({directory, owner, nonce: record.spawnNonce, pid: record.pid, startTicks: record.startTicks})
}

function exists(file) {
  try { fs.lstatSync(file); return true } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

export function inspectGeneration(generation) {
  // A missing procfs is an error, not proof of process retirement.
  fs.statSync('/proc/self/stat')
  let running = false
  try {
    const stat = fs.readFileSync(`/proc/${generation.pid}/stat`, 'utf8')
    const close = stat.lastIndexOf(')')
    assert.ok(close >= 0, 'Invalid process stat')
    const fields = stat.slice(close + 1).trim().split(/\s+/)
    assert.ok(fields.length > 19 && /^[0-9]+$/.test(fields[19]), 'Invalid process identity')
    running = fields[0] !== 'Z' && BigInt(fields[19]) === BigInt(generation.startTicks)
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
  const files = {
    lock: exists(path.join(generation.directory, 'backend.lock.json')),
    token: exists(path.join(generation.directory, `${generation.nonce}.token`)),
    log: exists(path.join(generation.directory, `${generation.nonce}.log`)),
  }
  return {running, files}
}

export function assertGenerationRetired(generation) {
  const state = inspectGeneration(generation)
  assert.equal(state.running, false, 'Owned process incarnation remains alive')
  assert.deepEqual(state.files, {lock: false, token: false, log: false}, 'Owned generation files remain')
}
