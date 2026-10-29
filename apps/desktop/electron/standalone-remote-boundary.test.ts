import assert from 'node:assert/strict'
import type { AddressInfo } from 'node:net'

import { test, vi } from 'vitest'
import { WebSocketServer } from 'ws'

import { buildGatewayWsUrl, buildGatewayWsUrlWithTicket } from './connection-config'
import { normalizeRegistry } from './connection-registry'
import { resolveDesktopRemoteRoute } from './desktop-remote-route'
import { probeGatewayWebSocket } from './gateway-ws-probe'
import { runPrimaryBackendStartup } from './primary-backend-startup'
import { createRegistryGatewayWsUrlHandler } from './remote-ws-headers'

for (const kind of ['remote', 'ssh'] as const) {
  for (const failureAt of ['resolve', 'connect'] as const) {
    test(`${kind} ${failureAt} failure never attaches to or starts a local Hermes`, async () => {
      const config = kind === 'ssh'
        ? { mode: 'ssh', remote: { mode: 'ssh', host: 'fixture.invalid', user: 'hermes' } }
        : { mode: 'remote', remote: { url: 'https://fixture.invalid', token: 'fixture-token' } }

      const failure = new Error(`${kind} fixture unavailable`)

      const options = {
        assertCurrentAttempt: () => {},
        attachHostBackend: vi.fn(async () => ({ local: true })),
        connectRemote: vi.fn(async () => { throw failure }),
        ensureLocalRuntime: vi.fn(async () => ({ local: true })),
        prepareLocalBackend: vi.fn(async () => ({ local: true })),
        resolveRemote: vi.fn(async () => {
          // Real routing runs before any transport I/O. An SSH dial error can
          // surface here because remote resolution includes bootstrapping.
          const route = resolveDesktopRemoteRoute({ config, registry: normalizeRegistry(null) })
          assert.equal(route?.kind, kind)

          if (failureAt === 'resolve') {
            throw failure
          }

          return route
        }),
        selectRegistryPrimary: vi.fn(async () => null),
        waitForDecision: vi.fn(async () => 'continue-local' as const),
        waitForLocalStart: vi.fn(async () => {})
      }

      await assert.rejects(runPrimaryBackendStartup(options), error => error === failure)
      assert.equal(options.connectRemote.mock.calls.length, failureAt === 'connect' ? 1 : 0)

      for (const localStep of [
        options.attachHostBackend,
        options.ensureLocalRuntime,
        options.prepareLocalBackend,
        options.selectRegistryPrimary,
        options.waitForDecision,
        options.waitForLocalStart
      ]) {
        assert.equal(localStep.mock.calls.length, 0)
      }
    })
  }
}

for (const accepted of [true, false]) {
  test(`URL token connection ${accepted ? 'accepts' : 'rejects'} credentials over a real WebSocket`, async () => {
    const server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
    await new Promise<void>((resolve, reject) => {
      server.once('listening', resolve)
      server.once('error', reject)
    })
    const requests: URL[] = []
    const baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/hermes`

    server.on('connection', (socket, request) => {
      const url = new URL(request.url!, baseUrl)
      requests.push(url)

      if (url.searchParams.get('token') === 'fixture-token') {
        socket.send(JSON.stringify({ jsonrpc: '2.0', method: 'gateway.ready' }))
      } else {
        socket.close(4401, 'fixture credential rejected')
      }
    })

    try {
      const mintTicket = vi.fn(async () => 'unused')

      const handler = createRegistryGatewayWsUrlHandler({
        ensureBackend: async () => ({
          authMode: 'token',
          baseUrl,
          profile: 'workbot',
          sharedRemote: true,
          wsUrl: buildGatewayWsUrl(baseUrl, accepted ? 'fixture-token' : 'wrong-fixture-token')
        }),
        mintTicket,
        buildTicketUrl: buildGatewayWsUrlWithTicket,
        rememberHeaders: () => {}
      })

      const wsUrl = await handler({ connectionId: 'remote-fixture', profile: 'workbot' })

      const result = await probeGatewayWebSocket(wsUrl, {
        WebSocketImpl: globalThis.WebSocket,
        connectTimeoutMs: 2000,
        readyGraceMs: 500
      })

      assert.equal(result.ok, accepted)
      assert.equal(mintTicket.mock.calls.length, 0)
      assert.equal(requests.length, 1)
      assert.equal(requests[0].pathname, '/hermes/api/ws')
      assert.equal(requests[0].searchParams.get('profile'), 'workbot')

      if (!accepted) {
        assert.match(result.reason, /4401/)
        assert.match(result.reason, /fixture credential rejected/)
      }
    } finally {
      for (const socket of server.clients) {
        socket.terminate()
      }

      await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()))
    }
  })
}
