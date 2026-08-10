/**
 * Playwright end-to-end test configuration.
 *
 * These specs drive the app through a real browser against the Vite dev server
 * rather than through the packaged Tauri window, so anything depending on
 * native desktop APIs is out of scope here and is covered by the Rust tests.
 *
 * Local and CI runs are tuned differently throughout: CI retries flaky specs,
 * runs them serially, and refuses to accept `.only` left behind in a commit,
 * whereas local runs favour speed and fast feedback.
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,

  // A stray `.only` would silently green the suite by skipping everything else,
  // so on CI it is treated as a hard failure.
  forbidOnly: !!process.env.CI,

  // Retry only on CI, where flakiness is usually timing rather than a real bug.
  retries: process.env.CI ? 2 : 0,

  // Single worker on CI keeps the shared dev server and its port contention
  // predictable; locally Playwright picks a worker count from the CPU.
  workers: process.env.CI ? 1 : undefined,

  reporter: 'html',
  use: {
    // Matches the fixed dev-server port pinned in vite.config.ts.
    baseURL: 'http://localhost:1420',
    // Traces are captured only when a test fails and is retried, which keeps
    // the artifact size down on green runs.
    trace: 'on-first-retry',
  },
  webServer: {
    // Playwright boots the dev server itself and waits for the URL to answer.
    command: 'npm run dev',
    url: 'http://localhost:1420',
    // Locally, attach to a server the developer already has running instead of
    // starting a competing one; CI always gets a clean instance.
    reuseExistingServer: !process.env.CI,
  },
  // Single browser target: the Tauri webview is Chromium-based, so testing
  // other engines would exercise behaviour the shipped app never encounters.
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ]
});
