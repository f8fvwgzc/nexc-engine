import { defineConfig, devices } from '@playwright/test';

const PORT = 4174;
const CI = !!process.env.CI;

/**
 * The screenshot baselines are rendered on Linux, in the pinned Playwright image that CI runs
 * (`make e2e-ui` uses it too). Anywhere else font rasterisation differs, so a run on another
 * system checks everything but the pixels, unless E2E_SNAPSHOTS=1 asks for them anyway.
 */
const compareSnapshots = process.platform === 'linux' || process.env.E2E_SNAPSHOTS === '1';

export default defineConfig({
  testDir: './e2e',
  outputDir: './e2e/.results',
  fullyParallel: true,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  workers: CI ? 2 : undefined,
  reporter: CI ? [['list'], ['html', { open: 'never', outputFolder: 'e2e/.report' }]] : 'list',
  ignoreSnapshots: !compareSnapshots,
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  expect: {
    timeout: 10_000,
    toHaveScreenshot: {
      // A pixel counts as changed past a small colour distance; a page fails past 1% of them.
      threshold: 0.2,
      maxDiffPixelRatio: 0.01,
      animations: 'disabled',
      caret: 'hide',
      scale: 'css',
    },
  },
  use: {
    baseURL: `http://localhost:${PORT}`,
    locale: 'en-US',
    timezoneId: 'UTC',
    colorScheme: 'light',
    reducedMotion: 'reduce',
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'ui',
      testMatch: ['ui.spec.ts', 'fixtures.spec.ts'],
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'audit',
      testMatch: 'audit/audit.spec.ts',
      timeout: 60_000,
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'audit-summary',
      testMatch: 'audit/summary.spec.ts',
      dependencies: ['audit'],
    },
  ],
  webServer: {
    // The production build, served as nginx would serve it (vite preview falls back to index.html).
    command: `pnpm exec vite build --logLevel warn && pnpm exec vite preview --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !CI,
    timeout: 240_000,
  },
});
