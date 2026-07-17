import { test, expect } from './fixtures/tauriMock';

test.describe('Statement Management - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/statements');
  });

  test('should render upload zone and handle concurrent network failures gracefully', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();
    await expect(page.locator('h1:has-text("Statements")')).toBeVisible();
    await expect(page.locator('text="Upload a new statement"').or(page.locator('text="Upload Statement"'))).toBeVisible();
    
    // Simulate invalid file upload (drag & drop non-PDF)
    // Note: since we can't easily mock actual native file drag without a file, we simulate the drop event
    const dataTransfer = await page.evaluateHandle(() => {
      const dt = new DataTransfer();
      const file = new File(['hello'], 'invalid.txt', { type: 'text/plain' });
      dt.items.add(file);
      return dt;
    });

    await page.dispatchEvent('[data-testid="dropzone"]', 'drop', { dataTransfer });

    // UI should show an error, not process it. TASK-FE-012 fix: was an
    // exact-match `text="Only PDF files are allowed"` locator, but the real
    // toast text has a trailing period ("...allowed.") -- Playwright's
    // quoted text= is whole-string equality, so this never matched.
    await expect(page.getByText(/Only PDF files are allowed/i).or(page.getByText(/Invalid file type/i)).first()).toBeVisible();
  });

  test('should rigorously handle password prompt: timeouts, incorrect retry, and success', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();
    // TASK-FE-012 fix: dispatched the illustrative dotted event name
    // ('statement.password_required'), but GlobalStateContext's real
    // listener is registered under the actual snake_case event string
    // ('statement_password_required', AppEvent::StatementPasswordRequired
    // in events.rs) -- the dispatch found no matching entry in
    // window.__TAURI_LISTENERS__ and silently no-opped, so this test never
    // actually opened the password modal before now.
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: { event: 'statement_password_required', payload: { statement_id: 'stmt_123' } }
      }));
    });
    
    const modalTitle = page.locator('h2:has-text("Password Required")').first();
    const unlockBtn = page.locator('button:has-text("Unlock & Parse")').first();
    await expect(modalTitle).toBeVisible();
    
    // Check countdown timer is visible and actively counting down from 2:30
    await expect(page.locator('text="2:30"').or(page.locator('text="2:29"'))).toBeVisible();
    
    // Incorrect password (simulate backend failure)
    await page.evaluate(() => { (window as any).__MOCK_STATE__.password_failure = true; });
    
    await page.fill('input[type="password"]', 'wrong');
    await page.click('button:has-text("Unlock & Parse")');
    
    // Must re-prompt WITHOUT closing the modal. TASK-FE-012 fix: the mock
    // now resolves a real attempts_remaining (matching the actual backend
    // contract), so the message is "Incorrect password — N attempts
    // remaining", not the bare exact string this used to check for.
    await expect(page.getByText(/Incorrect password/i)).toBeVisible();
    await expect(modalTitle).toBeVisible();
    
    // Simulate successful password
    await page.evaluate(() => { (window as any).__MOCK_STATE__.password_failure = false; });
    await page.fill('input[type="password"]', 'correct');
    await page.click('button:has-text("Unlock & Parse")');
    
    // Modal should close on success
    await expect(modalTitle).not.toBeVisible();
  });

  test('shows a queued/parsing progress indicator for large batches (statement_batch_progress)', async ({ page }) => {
    // Area 9 verification-pass fix: statement_batch_progress (queues.rs's
    // BatchProgressTracker, real backend signal for batches over the
    // 5-concurrent-parser cap) had zero frontend listeners -- large batches
    // showed a static "Uploading…" with no feedback for the whole wait.
    await expect(page.getByText('Drag and drop your PDF statements here')).toBeVisible();

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: { event: 'statement_batch_progress', payload: { parsed: 3, total: 12, eta_seconds: 45 } },
      }));
    });
    await expect(page.getByText(/Parsing 3 of 12 statements/i)).toBeVisible();
    await expect(page.getByText(/~45s remaining/i)).toBeVisible();

    // Clears once the batch finishes (parsed >= total).
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', {
        detail: { event: 'statement_batch_progress', payload: { parsed: 12, total: 12, eta_seconds: 0 } },
      }));
    });
    await expect(page.getByText(/Parsing \d+ of \d+ statements/i)).not.toBeVisible();
    await expect(page.getByText('Drag and drop your PDF statements here')).toBeVisible();
  });

  test('should display UnprocessedItemsQueue UI for retries on timeout', async ({ page }) => {
    // TASK-FE-012: UnprocessedItemsQueue now reads the real TASK-STMT-010
    // statements_list_unprocessed backend (3 status buckets) instead of the
    // old page deriving its own list from statement history. A password-entry
    // timeout maps to the "awaiting_password" bucket (still needs a
    // password, just re-prompted), not a generic retry.
    //
    // Seed the mock's backing state on the already-loaded page (page.evaluate
    // reaches the live window fine) and remount the query via *in-app*
    // navigation rather than a browser-level page.goto/reload -- a hash-only
    // goto() to the same route doesn't trigger a real navigation (so
    // addInitScript overrides never re-run), and reload() re-runs the
    // fixture's own addInitScript too, wiping the seeded state right back to
    // empty. useUnprocessedStatements' refetchOnMount:'always' (this task)
    // means remounting the page picks the seeded data up correctly.
    await expect(page.locator('aside')).toBeVisible();
    await page.evaluate(() => {
      window.__MOCK_STATE__.unprocessed_statements = {
        awaiting_password: [{ statement_id: 'stmt_123', filename: 'HDFC_Statement.pdf', failure_type: 'password_timeout', failure_reason: 'Password entry window expired' }],
        pending_retry: [],
        failed: [],
      };
    });
    await page.locator('a[href="#/instruments"]').click();
    await page.locator('a[href="#/statements"]').click();

    await expect(page.locator('text="Unprocessed Statements"')).toBeVisible();
    const retryBtn = page.locator('button:has-text("Enter Password")').first();
    await expect(retryBtn).toBeVisible();

    // Clicking it should open the password modal.
    await retryBtn.click();
    await expect(page.locator('h2:has-text("Password Required")').or(page.locator('h2:has-text("Unlock Statement")')).first()).toBeVisible();
  });

  // Doc 30 TASK-DESK-004 acceptance: `test_file_access_denial_shows_dismissable_toast`.
  // A macOS TCC file-access denial is a soft-fail for this specific upload
  // attempt -- a dismissable toast, not the blocking overlay Keychain
  // denial gets (that's a hard-fail, tested separately in AppShell.spec.ts).
  // A prior version of this handling opened a non-dismissable-by-default
  // modal dialog instead of a toast; fixed as part of this task.
  test('should show a dismissable toast (not a blocking modal) on file access denial', async ({ page }) => {
    await page.evaluate(() => {
      window.__MOCK_STATE__.file_access_denied = true;
    });

    const dataTransfer = await page.evaluateHandle(() => {
      const dt = new DataTransfer();
      const file = new File(['%PDF-1.5'], 'statement.pdf', { type: 'application/pdf' });
      dt.items.add(file);
      return dt;
    });
    await page.dispatchEvent('[data-testid="dropzone"]', 'drop', { dataTransfer });

    const toast = page.getByText(/File Access Denied/i).first();
    await expect(toast).toBeVisible();
    await expect(page.getByText(/System Settings.*Privacy.*Files and Folders/i).first()).toBeVisible();

    // Not a blocking dialog -- the rest of the page must stay interactive.
    await expect(page.locator('[role="dialog"]')).not.toBeVisible();
    await expect(page.locator('h1:has-text("Statements")')).toBeVisible();
  });
});
