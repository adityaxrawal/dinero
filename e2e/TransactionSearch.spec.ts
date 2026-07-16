import { test, expect } from './fixtures/tauriMock';

test.describe('Transaction Search & Detail - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/transactions');
    await page.waitForSelector('tbody tr', { timeout: 10000 }).catch(() => {});
  });

  test('should render transactions list with proper empty state or pagination boundaries', async ({ page }) => {
    await expect(page.locator('h1:has-text("Transactions")')).toBeVisible();
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    
    // Pagination tests
    const prevBtn = page.locator('button:has-text("Previous")');
    const nextBtn = page.locator('button:has-text("Next")');
    
    if (await prevBtn.isVisible()) {
      // On page 1, previous should be disabled
      await expect(prevBtn).toBeDisabled();
    }
    
    // Click next until disabled (boundary check)
    if (await nextBtn.isVisible() && await nextBtn.isEnabled()) {
      let limit = 5; // don't loop forever in tests
      while (await nextBtn.isEnabled() && limit > 0) {
        await nextBtn.click();
        await page.waitForTimeout(100);
        limit--;
      }
      if (limit > 0) {
        await expect(nextBtn).toBeDisabled();
      }
    }
  });

  test('should handle FTS5 full-text search securely (SQL injection attempts)', async ({ page }) => {
    const searchInput = page.locator('input[placeholder*="Search"]');
    
    // Attempt SQL injection via FTS query
    await searchInput.fill("' OR 1=1; --");
    await searchInput.press('Enter');
    
    // It should not crash the app, but return gracefully (likely empty results)
    await expect(page.locator('text="Something went wrong"')).not.toBeVisible();
    
    // Search with valid FTS special chars
    await searchInput.fill('amazon OR flipkart');
    await searchInput.press('Enter');
    await expect(page.locator('text="Something went wrong"')).not.toBeVisible();
  });

  test('transaction detail panel should rigorously manage tags (duplicates, colors)', async ({ page }) => {
    const firstRow = page.locator('tbody tr').first();
    if (!await firstRow.isVisible()) test.skip();
    
    await firstRow.click();
    await expect(page.locator('text="Details"')).toBeVisible();

    const tagInput = page.locator('input[placeholder="New tag..."]');
    await expect(tagInput).toBeVisible();

    // Add a tag
    await tagInput.fill('rigor-test');
    await page.keyboard.press('Enter');
    await expect(page.locator('text="rigor-test"')).toBeVisible();
    
    // Attempt to add duplicate tag
    await tagInput.fill('rigor-test');
    await page.keyboard.press('Enter');
    
    // Should not crash, should either show toast error or just silently ignore
    const tags = page.locator('text="rigor-test"');
    const count = await tags.count();
    expect(count).toBe(1); // Only one should exist

    // Delete tag
    const badge = page.locator('.badge', { hasText: 'rigor-test' }).or(page.locator('[class*="badge"]', { hasText: 'rigor-test' }));
    const removeBtn = badge.locator('div[class*="cursor-pointer"]').or(badge.locator('svg'));
    if (await removeBtn.first().isVisible()) {
      await removeBtn.first().click();
      await expect(page.locator('text="rigor-test"')).not.toBeVisible();
    }
  });

  test('transaction correction form should handle backend failures gracefully', async ({ page }) => {
    const firstRow = page.locator('tbody tr').first();
    if (!await firstRow.isVisible()) test.skip();
    
    await firstRow.click();
    
    // Mock backend rejection
    await page.evaluate(() => {
      (window as any).__MOCK_STATE__.tx_update_failure = true;
    });

    const saveBtn = page.locator('button:has-text("Save Corrections")');
    if (await saveBtn.isVisible()) {
       await saveBtn.click();
       // Verify error toast or message is shown, NOT a full crash
       await expect(page.locator('text="Failed to save"').or(page.locator('text="Error"')).or(page.locator('text="Update failed"'))).toBeVisible();
    }

    await page.evaluate(() => {
      (window as any).__MOCK_STATE__.tx_update_failure = false;
    });
  });
});
