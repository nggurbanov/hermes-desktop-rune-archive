/** Native namespace sockets keep gateway credentials outside the WebView. */
import { ask } from './bridge'
const NativeWebSocket = window.WebSocket
const prefix = 'ws://hermes-native-plugin.invalid/'
const gatewayPrefix = 'ws://hermes-native-gateway.invalid/'
const sockets = new Map<string, NativePluginSocket>()
function encoded(bytes: Uint8Array) { const parts: string[] = []; for (let n = 0; n < bytes.length; n += 16384) parts.push(String.fromCharCode(...bytes.subarray(n, n + 16384))); return btoa(parts.join('')) }
class NativePluginSocket extends EventTarget {
  readonly url: string
  protocol = ''
  readonly extensions = ''
  readonly CONNECTING = 0; readonly OPEN = 1; readonly CLOSING = 2; readonly CLOSED = 3
  readyState = 0
  bufferedAmount = 0
  binaryType: BinaryType = 'blob'
  onopen: ((event: Event) => unknown) | null = null
  onmessage: ((event: MessageEvent) => unknown) | null = null
  onerror: ((event: Event) => unknown) | null = null
  onclose: ((event: CloseEvent) => unknown) | null = null
  private receipt: string
  private sent = Promise.resolve()
  constructor(url: string) {
    super(); this.url = url; this.receipt = new URL(url).pathname.slice(1); sockets.set(this.receipt, this)
    void ask('plugin-socket-open', { receipt: this.receipt }).catch(error => this.failure(error))
  }
  private event(event: Event, listener: ((event: any) => unknown) | null) { try { listener?.(event) } finally { this.dispatchEvent(event) } }
  failure(error: unknown) {
    if (this.readyState === 3) return
    this.event(new Event('error'), this.onerror)
    this.receive('close', { code: 1006, reason: String(error instanceof Error ? error.message : error).slice(0, 120), clean: false })
  }
  receive(type: string, data: any) {
    if (this.readyState === 3) return
    if (type === 'open') { if (this.readyState === 0) { this.protocol = data?.protocol ?? ''; this.readyState = 1; this.event(new Event('open'), this.onopen) }; return }
    if (type === 'error') { this.event(new Event('error'), this.onerror); return }
    if (type === 'message') {
      let body: string | Blob | ArrayBuffer = data.text
      if (typeof data.bytes_base64 === 'string') {
        const bytes = Uint8Array.from(atob(data.bytes_base64), character => character.charCodeAt(0))
        body = this.binaryType === 'arraybuffer' ? bytes.buffer : new Blob([bytes])
      }
      this.event(new MessageEvent('message', { data: body, origin: this.url }), this.onmessage)
      return
    }
    if (type === 'close') { this.readyState = 3; sockets.delete(this.receipt); this.event(new CloseEvent('close', { code: data.code ?? 1006, reason: data.reason ?? '', wasClean: data.clean === true }), this.onclose) }
  }
  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (this.readyState !== 1) throw new DOMException('The plugin stream is not open', 'InvalidStateError')
    const size = typeof data === 'string' ? new TextEncoder().encode(data).byteLength : data instanceof Blob ? data.size : data.byteLength
    if (size > 8 * 1024 * 1024) throw new DOMException('The plugin stream frame exceeds the size limit', 'QuotaExceededError')
    this.bufferedAmount += size
    this.sent = this.sent.then(async () => {
      const frame = typeof data === 'string' ? { text: data } : { bytes_base64: encoded(new Uint8Array(data instanceof Blob ? await data.arrayBuffer() : ArrayBuffer.isView(data) ? data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) : data)) }
      await ask('plugin-socket-send', { receipt: this.receipt, frame })
    }).catch(error => this.failure(error)).finally(() => { this.bufferedAmount -= size })
  }
  close(code = 1000, reason = '') {
    if (code !== 1000 && (code < 3000 || code > 4999)) throw new DOMException('Invalid close code', 'InvalidAccessError')
    if (new TextEncoder().encode(reason).length > 123) throw new DOMException('Close reason exceeds 123 bytes', 'SyntaxError')
    if (this.readyState >= 2) return
    this.readyState = 2
    void ask('plugin-socket-close', { receipt: this.receipt, code, reason }).catch(error => this.failure(error))
  }
}
window.WebSocket = new Proxy(NativeWebSocket, { construct(target, args) { const url = String(args[0]); return url.startsWith(prefix) || url.startsWith(gatewayPrefix) ? new NativePluginSocket(url) : Reflect.construct(target, args) } })
window.addEventListener('hermes-host-message', ((event: CustomEvent) => { const message = event.detail; if (message.type === 'plugin-socket') sockets.get(message.receipt)?.receive(message.event, message.data); if (message.type === 'close') for (const socket of sockets.values()) socket.close() }) as EventListener)
