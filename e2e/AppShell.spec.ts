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
    // TASK-FE-017 fix: an unscoped `text=Connected` substring-matches the
    // new StatementOnlyModeBanner's "Gmail sync isn't connected" copy too
    // (shown by default since the fixture's gmail_connected starts false)
    // — scope to the core-engine-status indicator specifically.
    await expect(page.getByTestId('core-engine-status').getByText('Connected')).toBeVisible();
    // Validate visual indicator (e.g., green dot)
    await expect(page.locator('.bg-green-500').first()).toBeVisible();
  });

  // TASK-FE-018 fix (found while adjacent, same bug class already fixed
  // repeatedly this session): AppLayout's real listener is registered
  // under the actual snake_case event name (db_corrupted,
  // AppEvent::DbCorrupted in events.rs), not the illustrative dotted name
  // this test used to dispatch -- the wait/dispatch never matched anything
  // real, even though the recovery UI itself was already correctly wired.
  test('should display CorruptedDatabaseRecovery UI on db_corrupted event', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    // Wait for AppLayout to dynamically load @tauri-apps/api/event and register the listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['db_corrupted']);

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: { event: 'db_corrupted', payload: {} }
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

  // TASK-FE-016 fix: this test previously dispatched an illustrative
  // dotted event name ('license.clock_skew') that AppLayout never actually
  // listened for -- the real backend event of that name
  // (AppEvent::LicenseClockSkew -> "license_clock_skew") is defined but
  // never emitted anywhere in the crate (grepped); the ad-hoc listener this
  // test was built against was 100% dead code, unreachable in production
  // regardless of the real lock cause (grace expiry, invalid JWT, etc, not
  // just clock skew). The real, reactive channel is useLicenseStore
  // mirroring the `license_state_changed` broadcast (Doc 30's own explicit
  // spec for this task), which is what LicenseLockOverlay now subscribes
  // to. Also: the old assertion that the sidebar leaves the viewport
  // directly contradicted Doc 30's own task text ("blocking write
  // interactions but explicitly still allowing navigation to read-only
  // views ... and to the reactivation flow") -- corrected to assert the
  // opposite, that navigation stays reachable.
  test('should display License Locked overlay on license_state_changed(LOCKED) and prevent dismissal, without blocking navigation', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: {
          event: 'license_state_changed',
          payload: { state: 'LOCKED', is_active: false, license_key_masked: null, plan_id: null, billing_interval: null, expiry_date: null, days_remaining: null },
        },
      }));
    });

    await expect(page.locator('text="License Locked"')).toBeVisible();

    // Should NOT have a close/dismiss button of any kind.
    await expect(page.locator('button:has-text("Close")')).toHaveCount(0);
    await expect(page.locator('button:has-text("Dismiss")')).toHaveCount(0);

    // Per spec: read-only navigation and the reactivation flow both stay
    // reachable -- the overlay covers the routed content pane, not the
    // sidebar.
    await expect(page.locator('aside')).toBeInViewport();
    await page.locator('button:has-text("Reactivate in Settings")').click();
    await expect(page).toHaveURL(/.*\/settings/);
  });

  test('License Locked overlay dismisses reactively when a background revalidation restores ACTIVE', async ({ page }) => {
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: {
          event: 'license_state_changed',
          payload: { state: 'LOCKED', is_active: false, license_key_masked: null, plan_id: null, billing_interval: null, expiry_date: null, days_remaining: null },
        },
      }));
    });
    await expect(page.locator('text="License Locked"')).toBeVisible();

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: {
          event: 'license_state_changed',
          payload: { state: 'ACTIVE', is_active: true, license_key_masked: null, plan_id: 'pro', billing_interval: 'monthly', expiry_date: null, days_remaining: null },
        },
      }));
    });
    await expect(page.locator('text="License Locked"')).not.toBeVisible();
  });

  test('Grace Period banner shows days remaining and a working Retry validation action', async ({ page }) => {
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: {
          event: 'license_state_changed',
          payload: { state: 'GRACE', is_active: true, license_key_masked: null, plan_id: 'pro', billing_interval: 'monthly', expiry_date: null, days_remaining: 4 },
        },
      }));
    });

    await expect(page.getByText(/grace period/i)).toBeVisible();
    await expect(page.getByText(/4 days remaining/i)).toBeVisible();

    await page.locator('button:has-text("Retry validation now")').click();
    // A successful refresh resolves ACTIVE via the mock; the banner should
    // clear reactively, same as the overlay.
    await expect(page.getByText(/grace period/i)).not.toBeVisible();
  });

  // TASK-FE-018 fix: real event names are task_started/task_completed
  // (snake_case, AppEvent::TaskStarted/TaskCompleted), not the illustrative
  // dotted names this previously dispatched.
  test('should display global background task indicator without blocking UI', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();

    // Wait for listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['task_started']);

    // Trigger start
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: { event: 'task_started', payload: { id: 'task_1', message: 'Syncing with remote vault...' } }
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
        detail: { event: 'task_completed', payload: { id: 'task_1' } }
      }));
    });

    // Indicator should disappear
    await expect(indicator).not.toBeVisible();
  });
  

});
