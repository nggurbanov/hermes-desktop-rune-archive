import { createBrowserDesktopPluginsBridge } from '@/lib/browser-desktop-plugins'
// The child never receives a gateway URL, token, cookie or native ticket.
export const pending = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void }>()
export function emit(value: unknown) { window.ipc.postMessage(JSON.stringify(value)) }
export function ask(kind: string, payload: unknown = {}, timeoutMs = 30000): Promise<any> {
  const id = crypto.randomUUID()
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => { pending.delete(id); reject(new Error('The native host request timed out; its outcome may be unknown')) }, Math.min(Math.max(timeoutMs, 1000), 86400000))
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value) }, reject: error => { clearTimeout(timer); reject(error) } })
    emit({ type: 'plugin-request', id, kind, payload, timeout_ms: timeoutMs })
  })
}
window.addEventListener('hermes-host-message', ((event: CustomEvent) => {
  const message = event.detail
  if (message.type !== 'plugin-response') return
  const request = pending.get(message.id)
  if (!request) return
  pending.delete(message.id)
  message.error ? request.reject(new Error(message.error)) : request.resolve(message.value)
}) as EventListener)
let owner: any = null
export function hydrateBridgeOwner(snapshot: any) { owner = snapshot }
async function connection(profile?: string, connectionId?: string) {
  if (!owner) throw new Error('The plugin owner is unavailable')
  return ask('source-descriptor', { profile: profile || owner.profile, connectionId: connectionId || owner.connection_id }, 60000)
}
const api = async (request: any) => {
  if (!request?.upload) return ask('api', request, request?.timeoutMs ?? 30000)
  const metadata = { ...request }; delete metadata.upload
  const frozen = structuredClone(metadata)
  const filename = request.upload.filename
  const contentType = request.upload.contentType ?? 'application/octet-stream'
  const bytes = new Uint8Array(request.upload.bytes).slice()
  if (bytes.length > 256 * 1024 * 1024) throw new Error('The plugin upload exceeds the 256 MiB limit')
  const transfer = crypto.randomUUID()
  try {
    await ask('upload-start', { transfer_id: transfer, total_bytes: bytes.length, filename, content_type: contentType, request: frozen })
    for (let offset = 0, index = 0; offset < bytes.length; offset += 8 * 1024 * 1024, index++) {
      const chunk = bytes.subarray(offset, Math.min(offset + 8 * 1024 * 1024, bytes.length))
      const pieces: string[] = []
      for (let start = 0; start < chunk.length; start += 16384) pieces.push(String.fromCharCode(...chunk.subarray(start, start + 16384)))
      await ask('upload-chunk', { transfer_id: transfer, index, bytes_base64: btoa(pieces.join('')) })
    }
    return await ask('api', { ...frozen, upload: { transfer_id: transfer } }, frozen.timeoutMs ?? 30000)
  } catch (error) {
    // The original failure remains authoritative if host cleanup also fails.
    await ask('upload-abort', { transfer_id: transfer }).catch(() => {})
    throw error
  }
}
Object.assign(window, {
  hermesDesktop: {
    version: 'Rune plugin compatibility',
    platform: /mac/i.test(navigator.platform) ? 'darwin' : /win/i.test(navigator.platform) ? 'win32' : 'linux',
    api,
    connections: { list: async () => { const metadata = await ask('registry-metadata'); return { connections: metadata.connections.map(({ attached, ...row }: any) => row), primary: metadata.primary } } },
    getProfileRoutes: async (_profiles: string[]) => {
      const metadata = await ask('registry-roster', {}, 60000)
      return metadata.connections.filter((source: any) => !source.error).flatMap((source: any) => source.profiles.map((row: any) => ({ connectionId: source.id, profile: row.name, targetProfile: row.name, mode: source.mode, ...(source.id === metadata.primary ? { primary: true } : {}) })))
    },
    getAgentRoster: async () => {
      const metadata = await ask('registry-roster', {}, 60000)
      return {
        primaryConnectionId: metadata.primary,
        agents: metadata.connections.filter((source: any) => !source.error).flatMap((source: any) => source.profiles.map((row: any) => ({ connectionId: source.id, connectionKind: source.kind, connectionLabel: source.label, profile: row.name, targetProfile: row.name, handle: row.name === 'default' ? 'hermes' : row.name, profileMetadata: { title: row.title, display_name: row.display_name, has_avatar: row.has_avatar, ui_meta: row.ui_meta } }))),
        sources: metadata.connections.map((source: any) => ({ connectionId: source.id, label: source.label, kind: source.kind, reachable: source.attached === true, ...(source.error ? {error: source.error} : source.attached ? {} : { error: 'connect-on-demand' }) })),
      }
    },
    getConnection: async (profile?: string) => connection(profile),
    getConnectionFor: async (route: any) => connection(route.profile, route.connectionId),
    getPluginWsUrl: async (request: unknown) => (await ask('plugin-socket-prepare', request)).url,
    getGatewayWsUrl: async (profile?: string) => (await ask('plugin-gateway-prepare', { profile: profile || owner?.profile, connectionId: owner?.connection_id })).url,
    getGatewayWsUrlFor: async (request: unknown) => (await ask('plugin-gateway-prepare', request)).url,
    openExternal: (url: string) => ask('open-external', { url }),
    writeClipboard: (text: string) => ask('clipboard', { text }),
    revealPath: (path: string) => ask('reveal-path', { path }),
    selectPaths: (options: unknown) => ask('select-paths', options, 600000),
    selectSavePath: (options: unknown) => ask('select-save-path', options, 600000),
    ...createBrowserDesktopPluginsBridge({ api, files: {
      readFileText: (path: string) => api({ path: `/api/fs/read?${new URLSearchParams({ path })}` }),
      readDir: (path: string) => api({ path: `/api/fs/list?${new URLSearchParams({ path })}` }),
    } } as any),
    watchPreviewFile: (path: string) => watch(path, false),
    watchDirectory: (path: string) => watch(path, true),
    stopPreviewFileWatch: stopWatch,
    onPreviewFileChanged: (listener: (event: { id: string }) => void) => { changes.add(listener); return () => changes.delete(listener) },
    nativeNotify: (input: unknown) => ask('notify', input),
  },
})
const storedKey = (key: string) => key.startsWith('hermes.plugin.') || ['hermes.desktop.pluginDecisions.v2', 'hermes.desktop.disabledPlugins.v1'].includes(key)
function storageSnapshot() {
  return Object.fromEntries(Object.keys(localStorage).filter(key => storedKey(key)).map(key => [key, localStorage.getItem(key)]))
}
for (const method of ['setItem', 'removeItem', 'clear'] as const) {
  const original = Storage.prototype[method]
  Storage.prototype[method] = function (...args: any[]) {
    const result = Reflect.apply(original, this, args)
    if (this === localStorage && (method === 'clear' || storedKey(String(args[0])))) emit({ type: 'plugin-storage', values: storageSnapshot() })
    return result
  } as any
}
window.addEventListener('error', event => emit({ type: 'plugin-error', message: event.message }))
window.addEventListener('unhandledrejection', event => emit({ type: 'plugin-error', message: String(event.reason?.message ?? event.reason) }))
declare global { interface Window { ipc: { postMessage(value: string): void } } }

const changes = new Set<(event: { id: string }) => void>()
const watches = new Map<string, number>()
async function watch(path: string, directory: boolean) {
  const id = crypto.randomUUID()
  const read = () => directory ? window.hermesDesktop.readDir(path) : window.hermesDesktop.readFileText(path)
  let previous = JSON.stringify(await read())
  let reading = false
  const timer = window.setInterval(async () => {
    if (reading) return
    reading = true
    try {
      const next = JSON.stringify(await read())
      if (next !== previous) { previous = next; for (const listener of changes) { try { listener({ id }) } catch (error) { console.error(error) } } }
    } catch (error) {
      const next = String(error)
      if (next !== previous) { previous = next; for (const listener of changes) { try { listener({ id }) } catch (error) { console.error(error) } } }
    } finally { reading = false }
  }, 5000)
  watches.set(id, timer)
  return { id }
}
async function stopWatch(id: string) { const timer = watches.get(id); if (timer !== undefined) clearInterval(timer); watches.delete(id) }
window.addEventListener('hermes-host-message', ((event: CustomEvent) => { if (event.detail.type === 'close') { for (const timer of watches.values()) clearInterval(timer); watches.clear(); changes.clear() } }) as EventListener)
