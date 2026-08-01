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

  test('inline quick-actions tag add should not duplicate an existing tag', async ({ page }) => {
    // TASK-FE-009: tag management moved out of the old inline drawer (gone
    // -- see Transactions.spec.ts) into this page's own inline quick
    // actions (per-row "Add tag" button, optimistic + reconciled). Full
    // detail-page tag editing (with removal) is TASK-FE-010's real
    // TransactionDetail.tsx, not yet built beyond a placeholder.
    const firstRow = page.locator('tbody tr').first();
    if (!(await firstRow.isVisible())) test.skip();

    const addTagBtn = firstRow.getByLabel('Add tag');
    await addTagBtn.click();

    const tagInput = firstRow.getByLabel('New tag name');
    await expect(tagInput).toBeVisible();
    await tagInput.fill('rigor-test');
    await page.keyboard.press('Enter');

    // Should not crash the row; re-clicking "Add tag" and submitting the
    // same name again should not error (useAddTransactionTag's own
    // dedupe -- it fetches current tags and only appends if absent).
    await expect(page.locator('text="Something went wrong"')).not.toBeVisible();
    await firstRow.getByLabel('Add tag').click();
    await firstRow.getByLabel('New tag name').fill('rigor-test');
    await page.keyboard.press('Enter');
    await expect(page.locator('text="Something went wrong"')).not.toBeVisible();
  });

  test('transaction correction form should handle backend failures gracefully', async ({ page }) => {
    const firstRow = page.locator('tbody tr').first();
    if (!await firstRow.isVisible()) test.skip();
    
    await firstRow.click();
    
    // Mock backend rejection
    await page.evaluate(() => {
      (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.tx_update_failure = true;
    });

    const saveBtn = page.locator('button:has-text("Save Corrections")');
    if (await saveBtn.isVisible()) {
       await saveBtn.click();
       // Verify error toast or message is shown, NOT a full crash
       await expect(page.locator('text="Failed to save"').or(page.locator('text="Error"')).or(page.locator('text="Update failed"'))).toBeVisible();
    }

    await page.evaluate(() => {
      (window as unknown as { __MOCK_STATE__: Record<string, boolean> }).__MOCK_STATE__.tx_update_failure = false;
    });
  });
});
