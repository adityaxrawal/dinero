import { test, expect } from './fixtures/tauriMock';

test.describe('Debug Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/debug');
  });

  test('renders global metrics on System Health tab', async ({ page }) => {
    await page.getByRole('button', { name: 'System Health' }).click();
    
    // Narrow to main content area to avoid sidebar link
    const main = page.locator('.min-h-\\[400px\\]');
    await expect(main.getByText('Transactions')).toBeVisible();
    await expect(main.getByText('350')).toBeVisible();
    
    await expect(main.getByText('Statements')).toBeVisible();
    await expect(main.getByText('12')).toBeVisible();
    
    await expect(main.getByText('Unresolved Clusters')).toBeVisible();
    await expect(main.getByText('2', { exact: true })).toBeVisible();
  });

  test('renders pipeline controls on Pipeline State tab', async ({ page }) => {
    // Pipeline State is default active tab
    await expect(page.getByRole('heading', { name: 'Pipeline Controls' })).toBeVisible();

    // G17 fix: relabeled from "Gmail Poll"/"Scan Queue" to disambiguate from
    // PDF statement processing and clarify these control the Gmail transaction
    // pipeline specifically.
    await expect(page.getByRole('heading', { name: 'Transaction Polling' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Pause Polling' })).toBeVisible();

    await expect(page.getByRole('heading', { name: 'Historical Scan Queue' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Resume Scan' })).toBeVisible();
  });

  test('renders audit logs tab', async ({ page }) => {
    await page.getByRole('button', { name: 'Audit Log' }).click();
    
    await expect(page.getByText('CLUSTER_RESOLVE')).toBeVisible();
    await expect(page.getByText('cluster-')).toBeVisible();
  });
});
