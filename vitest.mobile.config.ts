import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'
import { fileURLToPath } from 'node:url'
import { featureAliases } from './scripts/frontend-profile.mjs'

export default defineConfig({
  plugins: [vue()],
  resolve: { alias: featureAliases(fileURLToPath(new URL('.', import.meta.url)), 'mobile') },
  test: {
    environment: 'jsdom',
    include: ['src/__tests__/mobile*.test.ts'],
    passWithNoTests: false,
  },
})
