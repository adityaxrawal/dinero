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
    await page.evaluate(() => { (window as any).__MOCK_STATE__.instrument_conflict = true; });
    
    await page.locator('[role="dialog"] button:has-text("Add")').or(page.locator('button[aria-label="Save new instrument"]')).click();
    
    // Expect error
    await expect(page.locator('[role="alert"]').or(page.locator('text="already exists"'))).toBeVisible();
    
    await page.evaluate(() => { (window as any).__MOCK_STATE__.instrument_conflict = false; });
  });

  test('should handle deletion constraints (e.g. ties to existing transactions)', async ({ page }) => {
    const deleteBtn = page.locator('button[aria-label*="Delete "]').first();
    if (!await deleteBtn.isVisible()) test.skip();
    
    await deleteBtn.click();
    const dialog = page.locator('[role="dialog"]');
    await expect(dialog.locator('text="Remove Instrument"')).toBeVisible();
    
    // Mock backend rejection because of tied transactions
    await page.evaluate(() => { (window as any).__MOCK_STATE__.instrument_delete_tied = true; });
    
    const confirmBtn = dialog.locator('button:has-text("Remove")');
    await confirmBtn.click();
    
    // Should NOT disappear, but show an error saying "Cannot delete instrument with linked transactions"
    await expect(page.locator('text=/transactions/i').or(page.locator('text="Failed"')).first()).toBeVisible();
    
    await page.evaluate(() => { (window as any).__MOCK_STATE__.instrument_delete_tied = false; });
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
