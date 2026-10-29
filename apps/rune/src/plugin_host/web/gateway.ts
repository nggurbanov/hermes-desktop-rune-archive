/** The original client owns multiplexing, canonical mapping, replay and profile leases. */
import { HermesGateway } from '@/api/client'
import { canonicalOwnerProfile } from '@/api/canonical-protocol'
import { configureGatewayRegistry, closeSecondaryGateways } from '@/store/gateway'
import { $gatewayState } from '@/store/session'
import { emitGatewayEvent } from '@/contrib/events'
import { emit } from './bridge'

export const gateway = new HermesGateway()
let owner: any = null
let connecting: Promise<void> | null = null
let primary: { connectionId: string; profile: string } | null = null
const cursors = new Map<string, { epoch: string; seq: number }>()
export function feedEvent(event: any): boolean {
  const session = event.session_id
  if (typeof event.seq === 'number' && Number.isFinite(event.seq) && typeof session === 'string') {
    const key = `${event.connectionId}\0${event.profile}\0${session}`
    const epoch = typeof event.replay_epoch === 'string' ? event.replay_epoch : ''
    const previous = cursors.get(key)
    if (previous && previous.epoch === epoch && previous.seq >= event.seq) return false
    cursors.delete(key); cursors.set(key, { epoch, seq: event.seq })
    if (cursors.size > 256) cursors.delete(cursors.keys().next().value!)
  }
  return true
}
function deliver(event: any) {
  const profile = canonicalOwnerProfile(event) ?? event.profile ?? primary?.profile
  const connectionId = event.connectionId ?? primary?.connectionId
  if (typeof profile !== 'string' || typeof connectionId !== 'string') return
  const scoped = { ...event, profile, connectionId }
  if (feedEvent(scoped)) emitGatewayEvent(scoped)
}
const request = gateway.request.bind(gateway)
gateway.request = (method, params = {}, timeoutMs, signal) => request(method, { profile: primary?.profile ?? owner?.profile, ...params }, timeoutMs, signal)
gateway.onState(state => $gatewayState.set(state))
gateway.onAny(deliver)
export function connectPrimary(snapshot: any): Promise<void> {
  owner = snapshot
  if (gateway.connectionState === 'open') return Promise.resolve()
  if (connecting) return connecting
  configureGatewayRegistry({ activeConnectionId: () => owner?.connection_id ?? null, onEvent: deliver })
  connecting = (async () => {
    const url = await window.hermesDesktop.getGatewayWsUrlFor!({ profile: snapshot.profile, connectionId: snapshot.connection_id })
    if (typeof url !== 'string') throw new Error('The native gateway did not return an opaque socket receipt')
    primary = { connectionId: snapshot.connection_id, profile: snapshot.profile }
    await gateway.connect(url)
  })().finally(() => { connecting = null })
  return connecting
}
export function updateGatewayOwner(snapshot: any) { owner = snapshot }
export function closeGateways() { closeSecondaryGateways(); gateway.close() }
// This observer socket never owns the app's prompt UI. The original clients above
// deliver requests on their own retained channel, with their own response closures.
export function feedRequest(request: any) { emit({ type: 'plugin-server-response', response: { token: request.token, error: { code: -32601, message: 'The observer socket has no prompt UI' } } }) }
