import { test, expect } from './fixtures/tauriMock';

test.describe('Statement-Only Mode Fallback Banner (TASK-FE-017)', () => {
  test('shows when no Gmail account is connected, directing to Statement Upload', async ({ page }) => {
    // The fixture's default __MOCK_STATE__ has gmail_connected: false.
    await page.goto('/');

    await expect(page.getByText(/Gmail sync isn't connected/i)).toBeVisible();
    await page.locator('button:has-text("Upload Statements")').click();
    await expect(page).toHaveURL(/.*\/statements/);
  });

  test('is suppressed on the Statements page itself', async ({ page }) => {
    await page.goto('/#/statements');
    await expect(page.getByText(/Gmail sync isn't connected/i)).not.toBeVisible();
  });

  test('does not show when a Gmail account is connected', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => {
      window.__MOCK_STATE__.gmail_connected = true;
      window.__MOCK_STATE__.connected_accounts = [
        { email: 'active@gmail.com', account_id: 'acc_active', account_status: 'ACTIVE' },
      ];
    });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/"]').first().click();

    await expect(page.getByText(/Gmail sync isn't connected/i)).not.toBeVisible();
  });

  test('dismissing hides it, without blocking any other part of the app', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByText(/Gmail sync isn't connected/i)).toBeVisible();

    await page.locator('button[aria-label="Dismiss statement-only mode notice"]').click();
    await expect(page.getByText(/Gmail sync isn't connected/i)).not.toBeVisible();

    // The rest of the app remains fully usable.
    await page.locator('nav a:has-text("Transactions")').click();
    await expect(page).toHaveURL(/.*\/transactions/);
  });
});
