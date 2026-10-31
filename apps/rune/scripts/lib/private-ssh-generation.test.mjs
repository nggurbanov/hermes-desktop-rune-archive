import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import {captureGeneration, inspectGeneration, assertGenerationRetired} from './private-ssh-generation.mjs'

test('retirement requires the exact files and process incarnation to be gone', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rune-generation-test-'))
  const owner = '0123456789abcdef0123456789abcdef', nonce = '0123456789abcdef'
  const dir = path.join(root, '.hermes', 'desktop-ssh', owner)
  fs.mkdirSync(dir, {recursive: true})
  const child = spawn('sleep', ['30'])
  try {
    const stat = fs.readFileSync(`/proc/${child.pid}/stat`, 'utf8')
    const ticks = Number(stat.slice(stat.lastIndexOf(')') + 1).trim().split(/\s+/)[19])
    fs.writeFileSync(path.join(dir, 'backend.lock.json'), JSON.stringify({ownershipId: owner, spawnNonce: nonce, pid: child.pid, startTicks: ticks}))
    fs.writeFileSync(path.join(dir, `${nonce}.token`), 'private fixture')
    const generation = captureGeneration(root, owner)
    assert.equal(inspectGeneration(generation).running, true)
    assert.throws(() => assertGenerationRetired(generation))
    child.kill('SIGTERM')
    await new Promise(resolve => child.once('close', resolve))
    assert.throws(() => assertGenerationRetired(generation), /files remain/)
    fs.unlinkSync(path.join(dir, 'backend.lock.json'))
    fs.unlinkSync(path.join(dir, `${nonce}.token`))
    assertGenerationRetired(generation)
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
    fs.rmSync(root, {recursive: true, force: true})
  }
})

test('a reused PID is not the captured incarnation and malformed ownership is rejected', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rune-generation-test-'))
  const owner = '0123456789abcdef0123456789abcdef'
  const dir = path.join(root, '.hermes', 'desktop-ssh', owner)
  fs.mkdirSync(dir, {recursive: true})
  try {
    const stat = fs.readFileSync('/proc/self/stat', 'utf8')
    const ticks = Number(stat.slice(stat.lastIndexOf(')') + 1).trim().split(/\s+/)[19])
    const lock = path.join(dir, 'backend.lock.json')
    fs.writeFileSync(lock, JSON.stringify({ownershipId: owner, spawnNonce: '0123456789abcdef', pid: process.pid, startTicks: ticks + 1}))
    const generation = captureGeneration(root, owner)
    assert.equal(inspectGeneration(generation).running, false)
    fs.writeFileSync(lock, JSON.stringify({ownershipId: 'wrong', spawnNonce: '0123456789abcdef', pid: process.pid, startTicks: ticks}))
    assert.throws(() => captureGeneration(root, owner))
    assert.throws(() => captureGeneration(root, '../escape'))
  } finally {
    fs.rmSync(root, {recursive: true, force: true})
  }
})
