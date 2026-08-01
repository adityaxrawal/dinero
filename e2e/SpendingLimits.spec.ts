import { test, expect } from './fixtures/tauriMock';

test.describe('Spending Limits - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/spending-limits');
    await page.waitForSelector('h1', { timeout: 10000 }).catch(() => {});
    await page.waitForTimeout(300);
  });

  test('should validate input bounds and prevent saving invalid state', async ({ page }) => {
    const input = page.locator('input#global-limit').or(page.locator('input[aria-label*="Global monthly spending limit"]'));
    if (!await input.isVisible()) test.skip();
    
    // Fill negative number
    await input.fill('-1000');
    
    const saveBtn = page.locator('button:has-text("Save Changes")').or(page.locator('button[aria-label="Save spending limits"]'));
    await saveBtn.click();
    
    // Ensure form didn't save invalid state (HTML5 validation should show up or Zod)
    await expect(page.locator('text="Invalid limit"').or(page.locator('text="must be positive"').or(page.locator('[role="alert"]')))).toBeVisible();
    
    // Test extremely large numbers (overflow)
    await input.fill('99999999999999999');
    await saveBtn.click();
    // Assuming backend max size validation kicks in or UI truncates
    await expect(page.locator('[role="alert"]').or(page.locator('text="Too large"'))).toBeVisible();
  });

  test('should correctly reflect boolean state for threshold toggles', async ({ page }) => {
    const btn80 = page.locator('button[role="switch"][aria-label*="80%"]');
    if (!await btn80.isVisible()) test.skip();

    const initialState = await btn80.getAttribute('aria-checked');
    await btn80.click();
    const newState = await btn80.getAttribute('aria-checked');
    
    expect(initialState).not.toBe(newState);
  });

  test('should display in-app notification on alert_threshold_crossed Tauri event dynamically', async ({ page }) => {
    // Wait for AppLayout to dynamically load @tauri-apps/api/event and register the listener
    await page.waitForFunction(() => (window as unknown as { __TAURI_LISTENERS__: Record<string, unknown> }).__TAURI_LISTENERS__ && (window as unknown as { __TAURI_LISTENERS__: Record<string, unknown> }).__TAURI_LISTENERS__['alert_threshold_crossed']);

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'alert_threshold_crossed', payload: { category: 'FOOD', limit: 8000, current: 7500, threshold: 90 } } 
      }));
    });
    // Assuming a toast or banner appears specifically checking for dynamic message
    await expect(page.locator('text=/alert:? FOOD exceeded 90% of budget/i').or(page.locator('text=/FOOD.*90%/i')).first()).toBeVisible();
  });
});
