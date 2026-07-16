import { test, expect } from '@playwright/test';

test.describe('Privacy & Network Activity Settings', () => {
  test.beforeEach(async ({ page }) => {
    // Intercept default IPC calls for settings
    await page.route('*/**/api/ipc', async (route) => {
      const request = route.request();
      const postData = request.postDataJSON();
      if (postData?.cmd === 'list_connected_accounts') {
        await route.fulfill({ json: [] });
      } else if (postData?.cmd === 'settings_network_activity_list') {
        await route.fulfill({
          json: [
            {
              id: '1',
              timestamp: new Date().toISOString(),
              method: 'GET',
              domain: 'oauth2.googleapis.com',
              url_redacted: 'https://oauth2.googleapis.com/token?redacted',
              bytes_sent: 200,
              bytes_received: 1024,
              status_code: 200,
              secret_fields_masked: 'Authorization'
            }
          ]
        });
      } else {
        await route.continue();
      }
    });

    await page.goto('/#/settings');
  });

  test('should display outbound channel disclosure', async ({ page }) => {
    const disclosureTitle = page.locator('text=Outbound Channels Disclosure:');
    await expect(disclosureTitle).toBeVisible();
    // Doc 01 §10.4's 5 documented destinations, verbatim from OUTBOUND_CHANNEL_DISCLOSURE.
    await expect(page.locator('text=Gmail API')).toBeVisible();
    await expect(page.locator('text=Google OAuth servers')).toBeVisible();
    await expect(page.locator('text=Licensing Backend')).toBeVisible();
    await expect(page.locator('text=GitHub Releases')).toBeVisible();
    await expect(page.locator('text=Hugging Face')).toBeVisible();
    await expect(page.locator('text=No third-party analytics or crash-reporting services')).toBeVisible();
  });

  test('should load network activity table', async ({ page }) => {
    // Wait for the table row to appear
    await expect(page.locator('text=oauth2.googleapis.com')).toBeVisible();
    await expect(page.locator('text=GET')).toBeVisible();
    await expect(page.locator('text=200')).toBeVisible();
  });

  test('should display empty state when no activity', async ({ page }) => {
    // Override the mock to return empty array
    await page.route('*/**/api/ipc', async (route) => {
      const request = route.request();
      const postData = request.postDataJSON();
      if (postData?.cmd === 'settings_network_activity_list') {
        await route.fulfill({ json: [] });
      } else {
        await route.continue();
      }
    });

    // Need to trigger a refresh since it already loaded the previous mock on mount
    await page.locator('button:has-text("Refresh")').click();

    await expect(page.locator('text=No outbound requests recorded yet.')).toBeVisible();
  });

  test('should display error state if IPC fails', async ({ page }) => {
    // Override the mock to fail
    await page.route('*/**/api/ipc', async (route) => {
      const request = route.request();
      const postData = request.postDataJSON();
      if (postData?.cmd === 'settings_network_activity_list') {
        await route.fulfill({ status: 500, json: { error: 'IPC Error' } });
      } else {
        await route.continue();
      }
    });

    await page.locator('button:has-text("Refresh")').click();
    await expect(page.locator('text=IPC Error')).toBeVisible();
  });
});
