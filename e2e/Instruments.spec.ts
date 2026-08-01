import { test, expect } from './fixtures/tauriMock';

test.describe('Instrument Management - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/instruments');
    await page.waitForSelector('h1', { timeout: 10000 }).catch(() => {});
  });

  test('should rigorously enforce UNIQUE(type, issuer_name, masked_identifier) constraint', async ({ page }) => {
    await page.locator('button:has-text("Add Instrument")').click();
    
    // We assume HDFC Bank credit_card 1234 already exists in mock
    await page.locator('#add-issuer').fill('HDFC Bank');
    await page.locator('#add-masked').fill('1234');
    
    // Select type if possible
    const selectTrigger = page.locator('button[role="combobox"]');
    if (await selectTrigger.isVisible()) {
      await selectTrigger.click();
      await page.locator('text="Credit Card"').click();
    }
    
    // Mock a constraint failure response
    await page.evaluate(() => { (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.instrument_conflict = true; });
    
    await page.locator('[role="dialog"] button:has-text("Add")').or(page.locator('button[aria-label="Save new instrument"]')).click();
    
    // Expect error. TASK-FE-011 fix: was an exact-match `text="already
    // exists"` locator, but the real toast shows the backend's full message
    // ("An instrument with this identifier already exists for this
    // issuer.") as a sentence, not that exact isolated phrase -- Playwright's
    // quoted text= is whole-string equality, not substring, so this never
    // matched. Switched to a substring-tolerant getByText regex.
    await expect(page.locator('[role="alert"]').or(page.getByText(/already exists/i)).first()).toBeVisible();
    
    await page.evaluate(() => { (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.instrument_conflict = false; });
  });

  test('should handle deletion constraints (e.g. ties to existing transactions)', async ({ page }) => {
    // TASK-FE-011: delete moved from an inline list-row button + Radix
    // confirmation dialog to InstrumentDetail's page-level action, which
    // uses a native confirm()/ask() dialog (same pattern as
    // TransactionDetail's delete, TASK-FE-010) -- Playwright auto-dismisses
    // unhandled native dialogs by default, so this needs an explicit accept
    // handler that no prior test in this suite has needed.
    page.on('dialog', (dialog) => dialog.accept());

    const firstCard = page.locator('[role="button"]').first();
    if (!(await firstCard.isVisible())) test.skip();
    await firstCard.click();
    await expect(page).toHaveURL(/\/instruments\/.+/);

    // Mock backend rejection because of tied transactions
    await page.evaluate(() => { (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.instrument_delete_tied = true; });

    await page.locator('button:has-text("Delete Instrument")').click();

    // Should NOT navigate away, but show an error about linked transactions
    await expect(page.getByText(/linked transactions/i).or(page.getByText(/Failed/i)).first()).toBeVisible();
    await expect(page).toHaveURL(/\/instruments\/.+/);

    await page.evaluate(() => { (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.instrument_delete_tied = false; });
  });

  test('should show validation error on empty/boundary form submission', async ({ page }) => {
    await page.locator('button:has-text("Add Instrument")').click();
    
    const saveBtn = page.locator('[role="dialog"] button:has-text("Add")').last();
    await saveBtn.click();

    // Verify HTML5 or Zod validation traps it
    await expect(page.locator('text="required"').or(page.locator('[role="alert"]'))).toBeVisible();
    
    // Boundary check for masked identifier (e.g., > 10 chars)
    await page.locator('#add-issuer').fill('Valid Bank');
    await page.locator('#add-masked').fill('12345678901'); // Too long usually
    await saveBtn.click();
    
    // Ensure form didn't succeed if masked length is strictly constrained
    // We verify the dialog is still open
    await expect(page.locator('[role="dialog"]')).toBeVisible();
  });
});
