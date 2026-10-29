/** Workspace ownership stays in the native parent; these verbs send no prompt. */
import { host } from '@/sdk/index'
import { ask, emit } from './bridge'
host.openSession = async (storedSessionId, options = {}) => { await ask('open-session', { stored_session_id: storedSessionId, options }, options.hydrationTimeoutMs ?? 60000) }
host.newChat = (profile, options = {}) => { void ask('new-chat', { profile, options }, 60000).catch(error => emit({ type: 'plugin-error', message: String(error.message ?? error) })) }
const ensureAgent = host.ensureAgent
host.ensureAgent = async (connectionId, profile) => { await ask('ensure-agent', { connectionId, profile }, 60000); await ensureAgent(connectionId, profile); await ask('adopt-sdk-route', { connectionId, profile }) }
