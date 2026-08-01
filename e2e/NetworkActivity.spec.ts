import { test, expect } from './fixtures/tauriMock';

/**
 * Relocated from the old `PrivacySettings.spec.ts` (which despite its name
 * was entirely about this feature) -- Network Activity moved from
 * Settings > Privacy to Debug > Network Activity, matching what Document 11
 * §17 / Document 36 §3 already described (the Settings placement was what
 * had drifted from spec). Also updated to the real 5-field entry shape
 * (`id`/`channel`/`destination`/`bytes_transferred`/`occurred_at`) and the
 * new paginated `{ entries, meta }` response -- the old mock fixture data
 * used a stale 7-field shape (`timestamp`/`method`/`domain`/`url_redacted`/
 * `bytes_sent`/`bytes_received`/`status_code`) the real backend hasn't
 * returned since Document 19 §13.10's `settings_get_network_activity`
 * rewrite, so these tests were exercising a shape that no longer exists.
 */
function makeEntry(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: '1',
    channel: 'google_oauth',
    destination: 'oauth2.googleapis.com',
    bytes_transferred: 1224,
    occurred_at: new Date().toISOString(),
    ...overrides,
  };
}

test.describe('Debug > Network Activity', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/debug');
    await page.getByRole('button', { name: 'Network Activity' }).click();
  });

  test('should display outbound channel disclosure', async ({ page }) => {
    const disclosureTitle = page.getByText('Outbound Channels Disclosure:');
    await expect(disclosureTitle).toBeVisible();
    // Doc 01 §10.4's 5 documented destinations, verbatim from OUTBOUND_CHANNEL_DISCLOSURE.
    await expect(page.getByText('Gmail API')).toBeVisible();
    await expect(page.getByText('Google OAuth servers')).toBeVisible();
    await expect(page.getByText('Licensing Backend')).toBeVisible();
    await expect(page.getByText('GitHub Releases')).toBeVisible();
    await expect(page.getByText('Hugging Face')).toBeVisible();
    await expect(page.getByText('No third-party analytics or crash-reporting services')).toBeVisible();
  });

  test('should load network activity table', async ({ page }) => {
    await page.evaluate((entry) => {
      window.__MOCK_STATE__.network_activity = [entry];
    }, makeEntry());

    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('oauth2.googleapis.com', { exact: true })).toBeVisible();
    await expect(page.getByText('Google OAuth', { exact: true })).toBeVisible();
  });

  test('should display empty state when no activity', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.network_activity = []; });
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('No outbound requests recorded yet.')).toBeVisible();
  });

  test('should display error state if IPC fails', async ({ page }) => {
    await page.evaluate(() => { window.__MOCK_STATE__.network_activity_failure = true; });
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('Failed to fetch network activity.')).toBeVisible();
  });

  test('should paginate across pages', async ({ page }) => {
    await page.evaluate((entries) => {
      window.__MOCK_STATE__.network_activity = entries;
    }, Array.from({ length: 30 }, (_, i) =>
      makeEntry({ id: String(i), destination: `host-${i}.googleapis.com` }),
    ));
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('Page 1 of 2 (30 total)')).toBeVisible();
    await expect(page.getByText('host-0.googleapis.com', { exact: true })).toBeVisible();
    await expect(page.getByText('host-25.googleapis.com', { exact: true })).not.toBeVisible();

    await page.getByRole('button', { name: 'Next page' }).click();

    await expect(page.getByText('Page 2 of 2 (30 total)')).toBeVisible();
    await expect(page.getByText('host-25.googleapis.com', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Next page' })).toBeDisabled();

    await page.getByRole('button', { name: 'Previous page' }).click();
    await expect(page.getByText('Page 1 of 2 (30 total)')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Previous page' })).toBeDisabled();
  });
});
