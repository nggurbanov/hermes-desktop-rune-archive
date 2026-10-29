import assert from 'node:assert/strict'
import { test } from 'vitest'

import { createMacSigner } from './mac-sign.mjs'

test('desktop signing preserves native entitlements without an agent payload or Python', async () => {
  const policy = { entitlements: '/app/entitlements.plist', entitlementsInherit: '/app/inherit.plist', hardenedRuntime: true, ignore: file => file.endsWith('.txt') }
  const fileOptions = { hardenedRuntime: true }
  let signed = false
  const signer = createMacSigner(policy, {
    env: {},
    sign: async options => {
      signed = true
      assert.equal(options.batchCodesignCalls, false)
      assert.equal(options.ignore('/app/README.txt'), true)
      assert.equal(options.ignore('/app/native'), false)
      assert.equal(options.optionsForFile('/app'), fileOptions)
    }
  })
  await signer({ app: '/app', platform: 'darwin', identity: 'Test Identity' }, {
    helper: {
      getOptionsForFile: async (app, target, actual) => {
        assert.equal(app, '/app')
        assert.equal(target, 'mac')
        assert.deepEqual(actual, policy)
        return () => fileOptions
      }
    }
  })
  assert.equal(signed, true)
})
