import assert from 'node:assert/strict'

import { test } from 'vitest'

import { ExternalStrategy } from './external'

test('standalone app updates are manual and do not use a Hermes checkout', async () => {
  const strategy = new ExternalStrategy({ payload: 'standalone', source: 'local' })
  const status = await strategy.check()

  assert.equal(status.supported, false)
  assert.equal(status.reason, 'standalone')
  assert.match(status.message, /Update connected Hermes agents separately/)
  const result = await strategy.apply()
  assert.equal(result.manual, true)
  assert.match(result.message!, /hermes-desktop-rune\/releases/)
  assert.equal(result.command, undefined)
})
