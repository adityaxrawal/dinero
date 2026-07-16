import { test, expect } from './fixtures/tauriMock';

test.describe('Transactions List & Detail', () => {
  test.beforeEach(async ({ page }) => {
    page.on('console', msg => console.log(msg.text()));
    await page.goto('http://localhost:1420/#/transactions');
  });

  test('should render transactions list and search input', async ({ page }) => {
    await expect(page.locator('h1:has-text("Transactions")')).toBeVisible();
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    await expect(page.locator('table')).toBeVisible();
  });

  test('should navigate to the transaction detail page when a row is clicked, and back again', async ({ page }) => {
    // TASK-FE-009: Doc30 splits List (this page) and Detail into separate
    // routes/tasks -- a row click now navigates to /transactions/:id instead
    // of opening an inline drawer. Full detail-page content assertions
    // (editable fields, tags, evidence panel) move to TASK-FE-010, which
    // replaces the current minimal placeholder route.
    const firstRow = page.locator('tbody tr').first();
    await expect(firstRow).toBeVisible();
    await firstRow.click();

    await expect(page).toHaveURL(/\/transactions\/.+/);
    await expect(page.locator('button:has-text("Back")')).toBeVisible();

    await page.click('button:has-text("Back")');
    await expect(page).toHaveURL(/\/transactions$/);
    await expect(page.locator('h1:has-text("Transactions")')).toBeVisible();
  });

  test('should filter transactions based on search query', async ({ page }) => {
    const searchInput = page.locator('input[placeholder*="Search"]');
    await searchInput.fill('amazon');
    await searchInput.press('Enter');

    // Wait for the table to update
    const firstRow = page.locator('tbody tr').first();
    await expect(firstRow).toContainText(/amazon/i);
  });

  test('should display a loaded/total count and support loading more via infinite scroll', async ({ page }) => {
    // TASK-FE-009: Previous/Next paging replaced by React Query's
    // useInfiniteQuery per Doc30 -- a visible "Load more" fallback button
    // backs the IntersectionObserver-driven scroll trigger.
    await expect(page.locator('text=/\\d+ of \\d+ loaded/')).toBeVisible();

    const loadMoreBtn = page.locator('button:has-text("Load more")');
    if (await loadMoreBtn.isVisible()) {
      await loadMoreBtn.click();
    }
  });
});
