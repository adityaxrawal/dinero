import { test, expect } from './fixtures/tauriMock';

test.describe('Settings - Connected Accounts and Password Management (TASK-FE-015)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/settings');
  });

  test('a degraded account shows a status badge and a Reconnect action', async ({ page }) => {
    await page.evaluate(() => {
      window.__MOCK_STATE__.gmail_connected = true;
      window.__MOCK_STATE__.connected_accounts = [
        { email: 'active@gmail.com', account_id: 'acc_active', account_status: 'ACTIVE' },
        { email: 'expired@gmail.com', account_id: 'acc_degraded', account_status: 'degraded' },
      ];
    });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/settings"]').click();

    await expect(page.getByText('active@gmail.com').first()).toBeVisible();
    await expect(page.getByText('expired@gmail.com')).toBeVisible();
    await expect(page.getByText('Needs Reconnection')).toBeVisible();
    await expect(page.getByText(/Syncing has stopped/i)).toBeVisible();

    const reconnectBtn = page.locator('button:has-text("Reconnect")');
    await expect(reconnectBtn).toBeVisible();
    await reconnectBtn.click();
    // A successful reconnect flips the mock's oauth flag; the degraded
    // status should clear once the account list refreshes.
    await expect(page.getByText('Needs Reconnection')).not.toBeVisible();
  });

  test('stored statement passwords render with a Forget action and never show the password', async ({ page }) => {
    await expect(page.locator('h3:has-text("Stored Statement Passwords")')).toBeVisible();
    await expect(page.getByText('HDFC Bank')).toBeVisible();
    await expect(page.getByText(/1234/)).toBeVisible();
    await expect(page.getByText(/Used successfully 3 times/i)).toBeVisible();

    const body = await page.locator('body').innerText();
    expect(body).not.toContain('password123');

    const forgetBtn = page.locator('button:has-text("Forget")');
    await expect(forgetBtn).toBeVisible();
    await forgetBtn.click();

    await expect(page.getByText('No stored passwords yet.')).toBeVisible();
  });
});
