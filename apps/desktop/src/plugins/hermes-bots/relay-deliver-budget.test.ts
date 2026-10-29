import { describe, expect, it } from 'vitest'

import {
  RELAY_DELIVER_BACKEND_CEILING_MS,
  RELAY_DELIVER_SETTLEMENT_MARGIN_MS,
  RELAY_DELIVER_TIMEOUT_MS,
  RELAY_TURN_ATTEMPT_MS,
  RELAY_TURN_LOCK_WAIT_MS,
  RELAY_TURN_MAX_ATTEMPTS
} from './relay-budget'

describe('bot_relay.deliver settlement budget', () => {
  it('keeps the client deadline strictly greater than the backend ceiling', () => {
    // Strictly greater, not equal: a backend that answers at its own limit
    // still has to serialize and transport that answer.
    expect(RELAY_DELIVER_SETTLEMENT_MARGIN_MS, 'settlement margin must be positive').toBeGreaterThan(0)

    // The client reserves transport time after the supported backend ceiling.
    expect(RELAY_DELIVER_BACKEND_CEILING_MS).toBe(
      RELAY_TURN_LOCK_WAIT_MS + RELAY_TURN_ATTEMPT_MS * RELAY_TURN_MAX_ATTEMPTS
    )
    expect(RELAY_DELIVER_TIMEOUT_MS).toBe(RELAY_DELIVER_BACKEND_CEILING_MS + RELAY_DELIVER_SETTLEMENT_MARGIN_MS)
  })
})
