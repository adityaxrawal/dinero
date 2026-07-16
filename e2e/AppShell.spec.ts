import { test, expect } from './fixtures/tauriMock';

test.describe('AppShell & Navigation - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('should render the sidebar and core navigation items correctly', async ({ page }) => {
    const sidebar = page.locator('aside');
    await expect(sidebar).toBeVisible();
    await expect(page.locator('text=Dinero').first()).toBeVisible();

    const navItems = [
      // G16 fix: Spending Limits moved out of the top-level nav into a
      // Settings subsection — it's no longer a sidebar item.
      'Dashboard', 'Transactions', 'Instruments', 'Statements',
      'Reconciliation', 'Settings'
    ];
    for (const item of navItems) {
      const navLink = page.locator(`nav >> text="${item}"`);
      await expect(navLink).toBeVisible();
      // Verify href attribute is present
      const href = await navLink.getAttribute('href');
      expect(href).not.toBeNull();
    }
  });

  test('should highlight the active navigation item and navigate correctly', async ({ page }) => {
    const transactionsLink = page.locator('nav >> text="Transactions"');
    await transactionsLink.click();
    await expect(page).toHaveURL(/.*\/transactions/);
    // Strict Tailwind class check for active state — matches only the
    // non-hover active-state background, not the inactive item's
    // `hover:bg-accent` (a plain `bg-accent` substring check would false-match that).
    const activeStateClass = /(?<!hover:)bg-secondary\b|(?<!hover:)bg-gray-200\b|(?<!hover:)bg-accent\b|bg-\[#2563eb\]/;
    await expect(transactionsLink).toHaveClass(activeStateClass);

    // G16 fix: Spending Limits is now reached via Settings, not a direct nav link.
    const settingsLink = page.locator('nav >> text="Settings"');
    await settingsLink.click();
    await expect(page).toHaveURL(/.*\/settings/);
    await expect(settingsLink).toHaveClass(activeStateClass);
    // Transactions link should lose active state
    await expect(transactionsLink).not.toHaveClass(activeStateClass);
  });

  // G16 fix: a fresh page load (rather than chaining off the Transactions
  // navigation above) — Settings makes several concurrent data fetches on
  // mount, and this keeps the assertion isolated from unrelated flakiness
  // elsewhere in the app shell.
  test('Settings has a Spending Limits subsection that navigates to /spending-limits', async ({ page }) => {
    await page.goto('/#/settings');
    await page.locator('button:has-text("Manage Spending Limits")').click();
    await expect(page).toHaveURL(/.*\/spending-limits/);
  });

  test('should display healthy core engine status', async ({ page }) => {
    await expect(page.locator('text=Core Engine')).toBeVisible();
    await expect(page.locator('text=Connected')).toBeVisible();
    // Validate visual indicator (e.g., green dot)
    await expect(page.locator('.bg-green-500').first()).toBeVisible();
  });

  test('should display CorruptedDatabaseRecovery UI on db.corrupted event', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    // Wait for AppLayout to dynamically load @tauri-apps/api/event and register the listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['db.corrupted']);

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'db.corrupted', payload: {} } 
      }));
    });

    await expect(page.locator('text="Database Corrupted"')).toBeVisible();
    await expect(page.locator('text="The SQLite integrity check failed."')).toBeVisible();
    
    // Recovery button should be present
    const recoveryBtn = page.locator('button:has-text("Restore from Backup")').first();
    await expect(recoveryBtn).toBeVisible();

    // Ensure it overlays the app entirely
    await expect(page.locator('aside')).not.toBeInViewport();
  });

  test('should display LOCKED license state on license.clock_skew event and prevent dismissal', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    // Wait for listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['license.clock_skew']);

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'license.clock_skew', payload: {} } 
      }));
    });

    await expect(page.locator('text="License Locked"')).toBeVisible();
    await expect(page.locator('text="Clock skew detected."')).toBeVisible();

    // Should NOT have a close button
    const closeBtn = page.locator('button:has-text("Close")');
    await expect(closeBtn).toHaveCount(0);
    
    // Ensure it overlays the app entirely
    await expect(page.locator('aside')).not.toBeInViewport();
  });

  test('should display global background task indicator without blocking UI', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    // Wait for listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['task.started']);

    // Trigger start
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'task.started', payload: { id: 'task_1', message: 'Syncing with remote vault...' } } 
      }));
    });

    // Indicator should appear
    const indicator = page.locator('text="Syncing with remote vault..."');
    await expect(indicator).toBeVisible();
    
    // UI should STILL be interactive (not blocked)
    await page.locator('nav a:has-text("Settings")').click();
    await expect(page).toHaveURL(/.*\/settings/);

    // Trigger complete
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'task.completed', payload: { id: 'task_1' } } 
      }));
    });

    // Indicator should disappear
    await expect(indicator).not.toBeVisible();
  });
  

});
