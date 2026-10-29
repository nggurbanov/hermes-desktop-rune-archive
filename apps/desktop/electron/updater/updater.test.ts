// updater/updater.test.ts — resolution precedence + wire-shape contracts
// for the strategy layer. Pure DI: no Electron, no payload.

import { describe, expect, it } from 'vitest'

import { buildStampPayload } from '../../scripts/write-build-stamp.mjs'

import { resolveUpdaterMechanism } from './index'

describe('build stamp → update ownership', () => {
  const provenance = { commit: 'a'.repeat(40), branch: 'main', dirty: false, source: 'ci' }

  it.each(['win32', 'darwin', 'linux'] as const)('%s standalone artifacts delegate only desktop updates', platform => {
    const stamp: ReturnType<typeof buildStampPayload> = buildStampPayload(provenance, {}, platform)

    expect(stamp.updateMechanism).toBe('external')
    expect(stamp.payload).toBe('standalone')
    expect(stamp.tag).toBe(null)
    expect(stamp.distribution).toBe('desktop-app')
    expect(stamp).not.toHaveProperty('runtime')
    expect(stamp).not.toHaveProperty('store')
    expect(resolveUpdaterMechanism({ platform, updateMechanism: stamp.updateMechanism })).toBe('external')
  })

  it.each(['win32', 'darwin', 'linux'] as const)('unstamped %s development uses source updates', platform => {
    expect(resolveUpdaterMechanism({ platform, updateMechanism: undefined })).toBe(
      platform === 'win32' ? 'windows-handoff' : 'posix-handoff'
    )
  })
})
