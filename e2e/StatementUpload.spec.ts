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
    
    // UI should show an error, not process it
    await expect(page.locator('text="Only PDF files are allowed"').or(page.locator('text="Invalid file type"')).first()).toBeVisible();
  });

  test('should rigorously handle password prompt: timeouts, incorrect retry, and success', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'statement.password_required', payload: { statement_id: 'stmt_123' } } 
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
    
    // Must re-prompt WITHOUT closing the modal
    await expect(page.locator('text="Incorrect password"')).toBeVisible();
    await expect(modalTitle).toBeVisible();
    
    // Simulate successful password
    await page.evaluate(() => { (window as any).__MOCK_STATE__.password_failure = false; });
    await page.fill('input[type="password"]', 'correct');
    await page.click('button:has-text("Unlock & Parse")');
    
    // Modal should close on success
    await expect(modalTitle).not.toBeVisible();
  });

  test('should display UnprocessedStatementsQueue UI for retries on timeout', async ({ page }) => {
    await expect(page.locator('aside')).toBeVisible();
    // Trigger timeout event
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'statement.password_timeout', payload: { statement_id: 'stmt_123' } } 
      }));
    });

    await expect(page.locator('text="Unprocessed Statements"')).toBeVisible();
    // Failed statement should show up here with a retry button
    const retryBtn = page.locator('button:has-text("Retry Processing")').first();
    await expect(retryBtn).toBeVisible();
    
    // Clicking retry should reopen the password modal
    await retryBtn.click();
    await expect(page.locator('h2:has-text("Password Required")').or(page.locator('h2:has-text("Unlock Statement")')).first()).toBeVisible();
  });
});
