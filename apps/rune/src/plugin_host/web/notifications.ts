/** Functions remain in this WebView. IPC carries only notifyId and action ids. */
import { clearPluginNotifyHandlers, invokePluginNotifyActivate, invokePluginNotifyAction } from '@/store/native-notifications'
import { resolveHermesOpenPath } from '@/lib/hermes-open-target'
import { $pluginRecords } from '@/contrib/plugins-store'
import { emit } from './bridge'
export let activationHash: string | null = null
let ownerKey: string | null = null
export function consumeActivationHash(path: string): boolean {
  const own = activationHash === path; activationHash = null; return own
}
window.addEventListener('hermes-host-message', ((event: CustomEvent) => {
  const value = event.detail
  if (value.type === 'close' || value.type === 'plugin-load') { ownerKey = value.owner ? value.owner.connection_id + '\0' + value.owner.profile : null; clearPluginNotifyHandlers(); return }
  if (value.type === 'plugin-state') { const next = value.owner.connection_id + '\0' + value.owner.profile; if (ownerKey !== next) clearPluginNotifyHandlers(); ownerKey = next; return }
  if (value.type === 'plugin-notify-closed') { if (typeof value.notifyId === 'string' && value.notifyId) clearPluginNotifyHandlers(value.notifyId); return }
  if (value.type !== 'plugin-notify-activate') return
  try {
    const record = $pluginRecords.get()[value.pluginId]
    if (record && record.status !== 'loaded') return
    if (value.actionId) invokePluginNotifyAction(value.notifyId, value.actionId)
    else invokePluginNotifyActivate(value.notifyId)
    const path = resolveHermesOpenPath(value.activate)
    if (path) { activationHash = path; window.location.hash = path }
  } catch (error) { emit({type:'plugin-error',message:String(error)}) }
  finally { if (typeof value.notifyId === 'string' && value.notifyId) clearPluginNotifyHandlers(value.notifyId) }
}) as EventListener)
