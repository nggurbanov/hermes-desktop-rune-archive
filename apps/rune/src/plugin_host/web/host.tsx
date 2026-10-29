import './bridge'
import './socket'
import './actions'
import { consumeActivationHash } from './notifications'
import { emit, hydrateBridgeOwner } from './bridge'
import { gateway, feedEvent, feedRequest, connectPrimary, updateGatewayOwner, closeGateways } from './gateway'
import * as React from 'react'
import { createRoot } from 'react-dom/client'
import { useStore } from '@nanostores/react'
import { atom } from 'nanostores'
import { QueryClientProvider } from '@tanstack/react-query'
import { HashRouter } from 'react-router'
import { I18nProvider } from '@/i18n/context'
import { RootTooltipProvider } from '@/components/ui/tooltip'
import { Button } from '@/components/ui/button'
import builtinBots from '@/plugins/hermes-bots/plugin'
import { pluginSettingsEntries } from '@/contrib/settings-pages'
import { ThemeProvider } from '@/themes/context'
import { queryClient } from '@/lib/query-client'
import { loadRuntimePlugin, unloadRuntimePlugin, discoverRuntimePlugins, watchRuntimePlugins } from '@/contrib/runtime-loader'
import { registry, $registryVersion } from '@/contrib/registry'
import { ContribBoundary, ContribRender } from '@/contrib/react/boundary'
import { $pluginRecords, setPluginEnabled } from '@/contrib/plugins-store'
import { setApiRequestProfile, setApiRequestConnection } from '@/api/client'
import { $profiles, $activeGatewayProfile } from '@/store/profile'
import { $gateway, setPrimaryGateway, setPrimaryGatewayConnectionId } from '@/store/gateway'
import { $gatewayState, $connection, $currentCwd, $currentModel, $activeSessionId, $selectedStoredSessionId, $busy } from '@/store/session'
import { emitGatewayEvent } from '@/contrib/events'
import { $hapticsMuted } from '@/store/haptics'
import { $completionSoundVariantId, resolveCompletionSoundVariantId } from '@/store/completion-sound'
import '@/styles.css'

const areas = new Set<string>()
const registerMany = registry.registerMany
registry.registerMany = contributions => { contributions.forEach(contribution => areas.add(contribution.area)); return registerMany(contributions) }
let activeId: string | null = null
const $loadedPlugin = atom<string | null>(null)
let generation = 0
let boot: any = null
let proofSent = false
let primaryWired = false
function hydrate(snapshot: any) {
  if (!snapshot?.profile) throw new Error('The native owner profile is missing')
  boot = snapshot
  hydrateBridgeOwner(snapshot)
  if (typeof snapshot.sound_muted === 'boolean') $hapticsMuted.set(snapshot.sound_muted)
  if (typeof snapshot.sound_preset === 'number') $completionSoundVariantId.set(resolveCompletionSoundVariantId(snapshot.sound_preset))
  updateGatewayOwner(snapshot)
  setApiRequestProfile(snapshot.profile)
  setApiRequestConnection(snapshot.connection_id)
  $activeGatewayProfile.set(snapshot.profile)
  $profiles.set(snapshot.profiles ?? [])
  $gatewayState.set(gateway.connectionState as any)
  $connection.set({ profile: snapshot.profile, connectionId: snapshot.connection_id, mode: 'remote' } as any)
  $currentCwd.set(snapshot.cwd ?? '')
  $currentModel.set(snapshot.model ?? '')
  $activeSessionId.set(snapshot.runtime_session ?? null)
  $selectedStoredSessionId.set(snapshot.session ?? null)
  $busy.set(Boolean(snapshot.busy))
  if (!primaryWired) {
    setPrimaryGateway(gateway, snapshot.profile)
    setPrimaryGatewayConnectionId(snapshot.connection_id, 'remote')
    $gateway.set(gateway)
    primaryWired = true
  }
}
async function load(message: any) {
  const intent = ++generation
  hydrate(message.owner)
  await connectPrimary(message.owner)
  hydrate(message.owner)
  if (activeId) unloadRuntimePlugin(activeId)
  if (!message.source.trim()) {
    await window.hermesDesktop.desktopPluginsRoot?.()
    await discoverRuntimePlugins()
    watchRuntimePlugins()
    $loadedPlugin.set('installed-desktop-plugins')
    emit({ type: 'plugin-loaded', id: 'installed-desktop-plugins' })
    return
  }
  const builtin = message.source === 'builtin:hermes-bots'
  let source = message.source
  if (builtin) { (window as any).__HERMES_NATIVE_BOTS_PLUGIN__ = builtinBots; source = 'export default window.__HERMES_NATIVE_BOTS_PLUGIN__' }
  const id = await loadRuntimePlugin(source, builtin ? 'bundled-hermes-bots' : 'native-approved-source', { kind: builtin ? 'bundled' : 'disk', file: message.origin })
  if (intent !== generation) { if (id) unloadRuntimePlugin(id); return }
  if (!id) throw new Error('The desktop plugin failed to register. See its error entry.')
  activeId = id
  $loadedPlugin.set(id)
  emit({ type: 'plugin-loaded', id })
}
function App() {
  const loadedPlugin = useStore($loadedPlugin)
  useStore($registryVersion)
  const records = useStore($pluginRecords)
  const [area, setArea] = React.useState<string>('')
  const [settingsPage, setSettingsPage] = React.useState<string>('')
  const names = [...areas].filter(name => registry.getArea(name).some(contribution => contribution.render)).sort()
  const selected = names.includes(area) ? area : names[0]
  const contributions = selected ? registry.getArea(selected) : []
  const settings = pluginSettingsEntries({ contributions: registry.getArea('settings.plugins'), rows: [], configTitle: 'Agent settings' }).flatMap(entry => [{ key: entry.uid, title: entry.title, render: entry.render }, ...entry.children.map(child => ({ key: entry.uid + ':' + child.id, title: entry.title + ' · ' + child.title, render: child.render }))]).filter(entry => entry.render)
  const chosenSettings = settings.find(entry => entry.key === settingsPage) ?? settings[0]
  const visible = selected === 'settings.plugins' ? chosenSettings ? [{ id: chosenSettings.key, source: 'settings', render: chosenSettings.render }] : [] : contributions
  const labels: Record<string, string> = { primarySidebar: 'Sidebar', secondarySidebar: 'Secondary sidebar', panes: 'Panels', 'settings.plugins': 'Plugin settings', palette: 'Commands' }
  React.useEffect(() => { if ($loadedPlugin.get() && !proofSent && contributions.some(contribution => contribution.render)) { requestAnimationFrame(() => { proofSent = true; emit({ type: 'plugin-rendered', id: activeId, areas: names, text: document.getElementById('plugin-content')?.textContent?.slice(0, 500) }) }) } }, [loadedPlugin, contributions, names.join('|')])
  return <main className="h-screen flex flex-col bg-background text-foreground">
    <header className="flex items-center gap-3 border-b px-5 py-3"><strong>{activeId ? records[activeId]?.name ?? activeId : 'Desktop plugins'}</strong><Button variant="ghost" onClick={() => void discoverRuntimePlugins().catch(error => emit({ type: 'plugin-error', message: String(error) }))}>Rescan installed plugins</Button><select aria-label="Plugin surface" value={selected ?? ''} onChange={event => setArea(event.target.value)}>{names.map(name => <option key={name} value={name}>{labels[name] ?? name.replace(/([a-z])([A-Z])/g, '$1 $2').replaceAll('.', ' · ')}</option>)}</select></header>
    <div className="flex min-h-0 grow"><aside className="w-64 shrink-0 overflow-auto border-r p-4" aria-label="Desktop plugins">{Object.values(records).sort((a, b) => a.name.localeCompare(b.name)).map(record => <div key={record.id} className="mb-4"><strong>{record.name}</strong><p className="text-xs text-muted-foreground">{record.description || record.kind}</p><Button variant="ghost" size="sm" onClick={() => void setPluginEnabled(record.id, record.status !== 'loaded').catch(error => emit({ type: 'plugin-error', message: String(error) }))}>{record.status === 'loaded' ? 'Disable' : 'Enable'}</Button>{record.error && <p role="alert">{record.error}</p>}</div>)}</aside><section id="plugin-content" className="grow overflow-auto p-5">{selected === 'settings.plugins' && <select aria-label="Plugin settings page" value={chosenSettings?.key ?? ''} onChange={event => setSettingsPage(event.target.value)}>{settings.map(entry => <option key={entry.key} value={entry.key}>{entry.title}</option>)}</select>}{visible.map(contribution => contribution.render && <ContribBoundary key={`${contribution.source}:${contribution.id}`} id={contribution.id}><ContribRender render={contribution.render} /></ContribBoundary>)}{Object.values(records).filter(record => record.status === 'error').map(record => <p role="alert" key={record.id}>{record.name}: {record.error}</p>)}</section></div>
  </main>
}
createRoot(document.getElementById('root')!).render(<HashRouter><QueryClientProvider client={queryClient}><I18nProvider configClient={null}><ThemeProvider><RootTooltipProvider><App /></RootTooltipProvider></ThemeProvider></I18nProvider></QueryClientProvider></HashRouter>)
window.addEventListener('hermes-host-message', ((event: CustomEvent) => {
  const message = event.detail
  if (message.type === 'plugin-load') void load(message).catch(error => emit({ type: 'plugin-error', message: String(error.message ?? error) }))
  if (message.type === 'plugin-state') hydrate(message.owner)
  if (message.type === 'gateway-event' && feedEvent(message.event)) { $gatewayState.set('open'); emitGatewayEvent(message.event) }

  if (message.type === 'plugin-server-request') feedRequest(message.request)
  if (message.type === 'close') { generation++; if (activeId) unloadRuntimePlugin(activeId); closeGateways() }
}) as EventListener)
emit({ type: 'plugin-bootstrap-ready' })

window.addEventListener('hashchange', () => { const path=window.location.hash.replace(/^#/, ''); if (!consumeActivationHash(path)) emit({ type: 'plugin-navigate', path }) })
