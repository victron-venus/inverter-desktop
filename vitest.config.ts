import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'
import { fileURLToPath } from 'node:url'
import { featureAliases, resolveFrontendProfile } from './scripts/frontend-profile.mjs'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: featureAliases(fileURLToPath(new URL('.', import.meta.url)), resolveFrontendProfile()),
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/__tests__/**/*.test.ts'],
    exclude: ['src/__tests__/mobile*.test.ts'],
  },
})
