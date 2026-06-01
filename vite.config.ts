import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],
  base: './',

  build: {
    rolldownOptions: {
      output: {
        // Split heavy vendor libraries into separate chunks so no single chunk
        // exceeds Vite's 500 kB warning threshold.  ECharts pulls in zrender
        // (its canvas/SVG renderer) which alone accounts for ~175 kB, so we
        // keep the two in separate chunks.
        codeSplitting: {
          groups: [
            // zrender is an internal dependency of echarts (~175 kB).
            { name: 'zrender-vendor', test: /\/node_modules\/zrender\// },
            // echarts + vue-echarts wrapper (~408 kB after splitting zrender).
            { name: 'echarts-vendor', test: /\/node_modules\/(echarts|vue-echarts)\// },
          ],
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },
}))
