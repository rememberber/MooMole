import { defineConfig } from '@playwright/test'

const port = Number(process.env.MOOMOLE_E2E_PORT ?? 1431)
if (!Number.isInteger(port) || port < 1 || port > 65535)
  throw new Error('MOOMOLE_E2E_PORT must be an integer from 1 to 65535')
const url = `http://127.0.0.1:${port}`

export default defineConfig({
  testDir: './tests/e2e',
  use: {
    baseURL: url,
    viewport: { width: 1200, height: 900 },
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url,
    reuseExistingServer: false,
  },
})
