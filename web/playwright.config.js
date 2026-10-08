import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/browser', timeout: 120_000, workers: 1,
  use: { baseURL: 'http://127.0.0.1:4173/web/', headless: true, actionTimeout: 10000, launchOptions: { args: ['--enable-unsafe-swiftshader'] } },
  webServer: { command: 'node tests/serve.js', url: 'http://127.0.0.1:4173/web/', reuseExistingServer: false },
});
