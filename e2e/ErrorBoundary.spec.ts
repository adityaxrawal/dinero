import { test, expect } from './fixtures/tauriMock';

test.describe('Global Error Boundary (TASK-FE-018)', () => {
  test('shows a friendly screen with Reload and Export Diagnostic Bundle actions, not a raw stack trace', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => { window.__MOCK_STATE__.force_render_crash = true; });
    await page.locator('nav a:has-text("Settings")').click();

    await expect(page.getByText('Something went wrong')).toBeVisible();
    // No raw error message/stack trace text leaks into the friendly screen.
    await expect(page.getByText(/is not a function/i)).not.toBeVisible();

    const exportBtn = page.locator('button:has-text("Export Diagnostic Bundle")');
    await expect(exportBtn).toBeVisible();
    await exportBtn.click();
    await expect(page.getByText(/Saved locally to:/i)).toBeVisible();

    await expect(page.locator('button:has-text("Reload")')).toBeVisible();
  });
});
