import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:8899',
    trace: 'on-first-retry',
    headless: true,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: 'PORT=8899 NOTION_TOKEN=mock_token DATABASE_IDS=mock_db DATA_SOURCE_IDS=mock_ds cargo run',
    url: 'http://127.0.0.1:8899/caldav-ui',
    reuseExistingServer: !process.env.CI,
    timeout: 120 * 1000,
  },
});
