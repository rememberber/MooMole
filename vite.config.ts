import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [react()],
  resolve: { alias: { '@moomole/i18n': fileURLToPath(new URL('./src/i18n', import.meta.url)) } },
  clearScreen: false,
  optimizeDeps: { exclude: ['@moomole/i18n/jsx-runtime', '@moomole/i18n/jsx-dev-runtime'] },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/.cache/**', '**/src-tauri/**', '**/docs/**', '**/third-party/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: { target: 'safari14' },
  test: { include: ['src/**/*.test.ts'] },
})
