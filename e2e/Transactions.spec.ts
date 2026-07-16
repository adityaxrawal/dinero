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

  test('should open detail panel when a row is clicked and allow closing', async ({ page }) => {
    // Click the first row in the table body
    const firstRow = page.locator('tbody tr').first();
    await expect(firstRow).toBeVisible();
    await firstRow.click();

    // Verify detail panel opens
    const detailPanel = page.locator('div[role="dialog"]');
    await expect(detailPanel).toBeVisible();
    await expect(detailPanel.locator('div.font-semibold:has-text("Details")').or(detailPanel.locator('h3:has-text("Details")'))).toBeVisible();

    // Close the detail panel
    const closeButton = detailPanel.locator('button[aria-label="Close"]');
    await closeButton.click();
    
    // Check it's gone
    await expect(detailPanel).not.toBeVisible();
  });

  test('should filter transactions based on search query', async ({ page }) => {
    const searchInput = page.locator('input[placeholder*="Search"]');
    await searchInput.fill('amazon');
    await searchInput.press('Enter');

    // Wait for the table to update
    const firstRow = page.locator('tbody tr').first();
    await expect(firstRow).toContainText(/amazon/i);
  });

  test('should display pagination controls and allow navigating pages', async ({ page }) => {
    await expect(page.locator('button:has-text("Previous")')).toBeVisible();
    await expect(page.locator('button:has-text("Next")')).toBeVisible();
    
    // Attempt clicking next
    const nextBtn = page.locator('button:has-text("Next")');
    if (await nextBtn.isEnabled()) {
      await nextBtn.click();
    }
  });
});
