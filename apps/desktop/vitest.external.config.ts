import { defineConfig } from 'vitest/config'

/** Cross-language contracts are an explicit integration lane, not conditional unit-test skips. */
export default defineConfig({
  test: {
    name: 'external-runtime',
    environment: 'node',
    include: ['contracts/**/*.test.ts'],
    globalSetup: ['./vitest.external-runtime.ts', './vitest.run-tmp.ts'],
    fileParallelism: false,
    testTimeout: 30_000
  }
})
