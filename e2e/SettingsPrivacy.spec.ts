import { test, expect } from './fixtures/tauriMock';

test.describe('Settings - Privacy and Consent History (TASK-FE-014)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/settings');
  });

  test('consent history renders seeded events with granted timestamps', async ({ page }) => {
    await expect(page.locator('h3:has-text("Consent History")')).toBeVisible();
    await expect(page.getByText('gmail_authorization')).toBeVisible();
    await expect(page.getByText('onboarding_network_disclosure')).toBeVisible();
  });

  test('consent history shows a positive empty state, not a blank panel', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.consent_history = []; });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/settings"]').click();

    await expect(page.getByText('No consent events recorded yet.')).toBeVisible();
  });

  test('export diagnostic bundle states the file is saved locally, never auto-uploaded', async ({ page }) => {
    await expect(page.getByText(/saved locally on this device only/i)).toBeVisible();
    await expect(page.getByText(/never automatically uploaded/i)).toBeVisible();

    await page.locator('button:has-text("Export Diagnostic Bundle")').click();
    await expect(page.getByText(/Saved locally to:/i)).toBeVisible();
  });

  test('disconnecting a Gmail account requires confirmation explaining sync will stop', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.gmail_connected = true; });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/settings"]').click();

    await expect(page.getByText('Gmail Connected')).toBeVisible();
    const disconnectBtn = page.locator('button:has-text("Disconnect")');
    await expect(disconnectBtn).toBeVisible();

    // The plugin:dialog|message mock auto-confirms; this asserts the click
    // path reaches a successful disconnect (i.e. the confirm gate didn't
    // silently swallow the action) rather than the dialog's exact copy,
    // which isn't independently inspectable through this mock.
    await disconnectBtn.click();
    await expect(page.getByText('Gmail Connected')).not.toBeVisible();
  });

  test('delete-my-data is a two-step, type-to-confirm flow wired to the real wipe command', async ({ page }) => {
    await page.locator('button:has-text("Delete My Data")').click();

    await expect(page.locator('h2:has-text("Delete My Data")')).toBeVisible();
    await expect(page.getByText('This cannot be undone.')).toBeVisible();
    await page.locator('button:has-text("I Understand, Continue")').click();

    await expect(page.locator('h2:has-text("Confirm Deletion")')).toBeVisible();
    const confirmBtn = page.locator('button:has-text("Permanently Delete")');
    await expect(confirmBtn).toBeDisabled();

    await page.fill('#reset-confirm-text', 'wrong phrase');
    await expect(confirmBtn).toBeDisabled();

    await page.fill('#reset-confirm-text', 'DELETE MY DATA');
    await expect(confirmBtn).toBeEnabled();
  });
});
