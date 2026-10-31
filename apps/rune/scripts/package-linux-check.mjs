#!/usr/bin/env node
// Exercise an installed AppRun against an anonymous, loopback-only read fixture.
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { WebSocketServer } from 'ws'

const paths = process.argv.slice(2)
assert.ok(paths.length, 'Pass one or more installed AppRun/executable paths')
const server = createServer((req, res) => { res.writeHead(403); res.end() })
const ws = new WebSocketServer({server, path: '/api/ws'})
const calls = []
let failure
ws.on('connection', (socket, req) => {
  if (req.headers.authorization || new URL(req.url, 'http://localhost').searchParams.has('token')) {
    failure = new Error('Unexpected credential'); socket.close(); return
  }
  socket.on('message', bytes => {
    const frame = JSON.parse(bytes); calls.push(frame.method)
    const results = {'profiles.list': {profiles: [{name: 'default'}]}, 'session.list': {sessions: []}}
    if (!(frame.method in results)) { failure = new Error(`Forbidden RPC ${frame.method}`); socket.close(); return }
    socket.send(JSON.stringify({jsonrpc: '2.0', id: frame.id, result: results[frame.method]}))
  })
})
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
try {
  for (const path of paths) {
    calls.length = 0
    const env = {...process.env}; delete env.HERMES_GATEWAY_TOKEN
    const child = spawn(path, ['--check-connection', '--gateway', `http://127.0.0.1:${server.address().port}`], {env})
    let stdout = '', stderr = ''
    child.stdout.on('data', data => { stdout += data }); child.stderr.on('data', data => { stderr += data })
    const timer = setTimeout(() => child.kill(), 15000)
    const status = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', resolve) })
    clearTimeout(timer)
    assert.ifError(failure); assert.equal(status, 0, stderr)
    const result = JSON.parse(stdout.trim())
    assert.equal(result.connected, true); assert.equal(result.read_only, true)
    assert.deepEqual(calls, ['profiles.list', 'session.list'])
    console.log(`${path}: read-only loopback check passed ${JSON.stringify(result)}`)
  }
} finally {
  for (const client of ws.clients) client.terminate()
  await new Promise(resolve => ws.close(resolve)); await new Promise(resolve => server.close(resolve))
}
