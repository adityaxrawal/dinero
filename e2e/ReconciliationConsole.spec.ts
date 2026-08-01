import { test, expect } from './fixtures/tauriMock';

test.describe('Reconciliation Console - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/reconciliation');
    await page.waitForSelector('h1', { timeout: 10000 }).catch(() => {});
  });

  test('should render both queue sections with pending clusters and unassigned transactions', async ({ page }) => {
    await expect(page.locator('h1:has-text("Reconciliation")')).toBeVisible();
    await expect(page.getByText(/Resolve ambiguous and unassigned transactions/i)).toBeVisible();

    await expect(page.locator('h2:has-text("Pending Clusters")')).toBeVisible();
    await expect(page.locator('h2:has-text("Unassigned Transactions")')).toBeVisible();

    // TASK-FE-013: the fixture seeds 2 clusters and 1 unassigned transaction.
    await expect(page.getByText(/Ambiguous match: Same amount on same day/i)).toBeVisible();
    await expect(page.getByText(/issuer_name_not_found/i)).toBeVisible();
  });

  test('cluster detail page shows side-by-side comparison and resolves via confirm_match', async ({ page }) => {
    await page.locator('button:has-text("Review Cluster")').first().click();

    await expect(page.locator('h1:has-text("Ambiguous Match Cluster")')).toBeVisible();
    // TASK-FE-013 fix: no fabricated confidence badge -- Document 18 has no
    // such field on either reconciliation table, so it must never appear.
    await expect(page.getByText(/Confidence:/i)).toHaveCount(0);

    await expect(page.getByText('New Evidence')).toBeVisible();
    await expect(page.getByText('Existing Match A')).toBeVisible();

    const confirmBtn = page.locator('button:has-text("Confirm Match")');
    await expect(confirmBtn).toBeDisabled();

    await page.locator('text=Existing Match A').first().click();
    await expect(confirmBtn).toBeEnabled();
    await confirmBtn.click();

    await expect(page.getByText(/Cluster Resolved/i).first()).toBeVisible();
    await expect(page).toHaveURL(/#\/reconciliation$/);
  });

  test('resolution failure keeps the user on the detail page with an error toast', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.resolve_failure = true; });
    await page.locator('button:has-text("Review Cluster")').first().click();
    await page.locator('text=Existing Match A').first().click();
    await page.locator('button:has-text("Confirm Match")').click();

    await expect(page.getByText(/Resolution Failed/i).first()).toBeVisible();
    await expect(page.locator('h1:has-text("Ambiguous Match Cluster")')).toBeVisible();
  });

  test('empty state shows a positive "All Caught Up" confirmation, not a blank page', async ({ page }) => {
    await page.evaluate(() => {
      window.__MOCK_STATE__.no_clusters = true;
      window.__MOCK_STATE__.unassigned_transactions = [];
    });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/reconciliation"]').click();

    await expect(page.getByText(/All Caught Up/i)).toBeVisible();
  });
});
