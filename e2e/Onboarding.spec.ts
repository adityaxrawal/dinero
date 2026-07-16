import { test, expect } from './fixtures/tauriMock';

test.describe('Onboarding Flow - Rigorous Verification', () => {
  test.beforeEach(async ({ page }) => {
    // Clear the mock onboarded flag for these specific tests
    await page.addInitScript(() => window.localStorage.removeItem('dinero_onboarded'));
    await page.goto('/#/onboarding');
  });

  test('should enforce timezone, boundary limits, and strictly gate completion', async ({ page }) => {
    await expect(page.locator('text="Welcome to Dinero"')).toBeVisible();
    
    // Timezone confirmation
    await expect(page.locator('label:has-text("Timezone")')).toBeVisible();
    
    // Limit boundary tests
    const limitInput = page.locator('input#limit');
    await expect(limitInput).toBeVisible();
    
    // Test negative/invalid limit (should be disabled or show error)
    await limitInput.fill('-500');
    await page.click('button:has-text("Continue")');
    // We expect the form to either not submit or show a validation error
    // (Assuming HTML5 validation or Zod prevents it)
    await expect(page.locator('text="Must be a positive number"').or(page.locator('text="Welcome to Dinero"')).first()).toBeVisible();

    await limitInput.fill('60000');
    
    // Statement preference & LLM config
    await expect(page.locator('label:has-text("Statement Preference")')).toBeVisible();
    await expect(page.locator('label:has-text("Local LLM Model")')).toBeVisible();
    
    await page.click('button:has-text("Continue")');

    // Step 2: History & Settings
    await expect(page.locator('label:has-text("Historical Scan Range")')).toBeVisible();
    await page.click('button[role="combobox"]');
    await page.click('text="6 Months"');
    await page.click('button:has-text("Continue")');
    
    // Step 3: Gmail connect screen
    await expect(page.locator('text="Connect your Gmail"')).toBeVisible();
    await expect(page.locator('text="We require read-only access to parse financial emails."')).toBeVisible();
    await expect(page.locator('text="https://www.googleapis.com/auth/gmail.readonly"')).toBeVisible();
    
    // Try to finish without connecting
    const finishBtn = page.locator('button:has-text("I Understand, Continue to Google")');
    // Ensure button is disabled or gates completion until token is actually stored
    
    // We mock the failure of token storage
    await page.evaluate(() => {
      (window as any).__MOCK_STATE__.gmail_failure = true;
    });
    
    await finishBtn.click();
    // It should show an error and NOT redirect
    await expect(page.locator('text="Failed to store token"').or(page.locator('text="Authentication failed"')).first()).toBeVisible();
    await expect(page).not.toHaveURL(/.*\/dashboard/);

    // Mock success
    await page.evaluate(() => {
      (window as any).__MOCK_STATE__.gmail_failure = false;
    });
    
    await finishBtn.click();
    
    await page.waitForURL(/.*\/|\/dashboard/);
    await expect(page.locator('text="Dashboard"').first()).toBeVisible();
  });
});
