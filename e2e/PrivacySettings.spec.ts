import { test, expect } from './fixtures/tauriMock';

/**
 * TASK-FE-014 fix: this file previously imported the stock `@playwright/test`
 * and mocked via `page.route('*​/**​/api/ipc', ...)` -- Tauri IPC is not a
 * real HTTP endpoint, it goes through `window.__TAURI_INTERNALS__.invoke`,
 * so that route interception never matched a real request and these tests
 * were exercising an unmocked, effectively empty environment. Also targeted
 * the pre-rename `settings_network_activity_list` command name. Converted to
 * the same `tauriMock` fixture every other spec in this suite uses.
 */
test.describe('Privacy & Network Activity Settings', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/settings');
  });

  test('should display outbound channel disclosure', async ({ page }) => {
    const disclosureTitle = page.getByText('Outbound Channels Disclosure:');
    await expect(disclosureTitle).toBeVisible();
    // Doc 01 §10.4's 5 documented destinations, verbatim from OUTBOUND_CHANNEL_DISCLOSURE.
    await expect(page.getByText('Gmail API')).toBeVisible();
    await expect(page.getByText('Google OAuth servers')).toBeVisible();
    await expect(page.getByText('Licensing Backend')).toBeVisible();
    await expect(page.getByText('GitHub Releases')).toBeVisible();
    await expect(page.getByText('Hugging Face')).toBeVisible();
    await expect(page.getByText('No third-party analytics or crash-reporting services')).toBeVisible();
  });

  test('should load network activity table', async ({ page }) => {
    await page.evaluate(() => {
      window.__MOCK_STATE__.network_activity = [
        {
          id: '1', timestamp: new Date().toISOString(), method: 'GET',
          domain: 'oauth2.googleapis.com', url_redacted: 'https://oauth2.googleapis.com/token?redacted',
          bytes_sent: 200, bytes_received: 1024, status_code: 200, secret_fields_masked: 'Authorization',
        },
      ];
    });
    // Multiple buttons on this page substring-match "Refresh" (e.g. "Refresh
    // License") -- NetworkActivity's own button's accessible name is
    // exactly "Refresh", nothing more.
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('oauth2.googleapis.com', { exact: true })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'GET' })).toBeVisible();
    await expect(page.getByRole('cell', { name: '200' }).first()).toBeVisible();
  });

  test('should display empty state when no activity', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.network_activity = []; });
    // Multiple buttons on this page substring-match "Refresh" (e.g. "Refresh
    // License") -- NetworkActivity's own button's accessible name is
    // exactly "Refresh", nothing more.
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('No outbound requests recorded yet.')).toBeVisible();
  });

  test('should display error state if IPC fails', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.network_activity_failure = true; });
    // Multiple buttons on this page substring-match "Refresh" (e.g. "Refresh
    // License") -- NetworkActivity's own button's accessible name is
    // exactly "Refresh", nothing more.
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('Failed to fetch network activity.')).toBeVisible();
  });
});
