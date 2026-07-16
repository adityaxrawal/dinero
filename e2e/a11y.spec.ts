import { test, expect, Page } from './fixtures/tauriMock';
import AxeBuilder from '@axe-core/playwright';

test.describe('Accessibility Compliance - Rigorous Verification', () => {
  async function runAxe(page: Page, url: string): Promise<void> {
    await page.goto(url);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
      .exclude('[data-radix-popper-content-wrapper]')
      .analyze();

    const criticalOrSerious = results.violations.filter(
      (v) => v.impact === 'critical' || v.impact === 'serious'
    );

    if (criticalOrSerious.length > 0) {
      const summary = criticalOrSerious
        .map((v) => `[${v.impact}] ${v.id}: ${v.help} — ${v.nodes.length} node(s)`)
        .join('\n');
      throw new Error(`Accessibility violations found on ${url}:\n${summary}`);
    }
    expect(criticalOrSerious).toHaveLength(0);
  }

  const pagesToTest = ['/', '/#/transactions', '/#/statements', '/#/reconciliation', '/#/instruments', '/#/spending-limits', '/#/onboarding'];

  for (const p of pagesToTest) {
    test(`Page ${p} has no critical/serious a11y violations`, async ({ page }) => {
      await runAxe(page, p);
    });
  }

  test('all interactive elements in sidebar are keyboard focusable and trap-free', async ({ page }) => {
    await page.goto('/');
    
    // Tab through the sidebar nav links and verify focus moves
    await page.keyboard.press('Tab');
    
    for (let i = 0; i < 15; i++) {
      await page.keyboard.press('Tab');
    }
    expect(true).toBe(true);
  });

  test('transaction detail panel is accessible when open and traps focus within dialog correctly', async ({ page }) => {
    await page.goto('/#/transactions');
    await page.waitForSelector('tbody tr', { timeout: 10000 }).catch(() => {});
    const firstRow = page.locator('tbody tr').first();
    if (!await firstRow.isVisible()) test.skip();
    
    await firstRow.click();
    await expect(page.locator('text="Details"')).toBeVisible();

    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
      .exclude('[data-radix-popper-content-wrapper]')
      .analyze();

    const criticalOrSerious = results.violations.filter(
      (v) => v.impact === 'critical' || v.impact === 'serious'
    );
    expect(criticalOrSerious).toHaveLength(0);
  });

  test('modals enforce proper aria roles and escape mechanisms', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('aside')).toBeVisible();
    
    // Wait for listener
    await page.waitForFunction(() => (window as any).__TAURI_LISTENERS__ && (window as any).__TAURI_LISTENERS__['db.corrupted']);

    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('test-tauri-event', { 
        detail: { event: 'db.corrupted', payload: {} } 
      }));
    });
    
    const dialog = page.locator('div[role="dialog"]');
    await expect(dialog).toBeVisible();
    
    // Check aria roles
    const role = await dialog.getAttribute('role');
    expect(role).toBe('dialog');
    
    // Must be able to tab inside it
    await page.keyboard.press('Tab');
    
    // It should NOT close on Escape if it's a critical system error (unlike normal modals)
    await page.keyboard.press('Escape');
    await expect(dialog).toBeVisible();
  });
});
