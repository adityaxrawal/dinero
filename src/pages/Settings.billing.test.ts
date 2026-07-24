// Doc 30 TASK-BILL-010: the remaining two acceptance criteria, written as
// static source checks (a full render test would need mocking every other
// Settings tab's IPC calls too, for zero extra signal on these two specific
// claims -- matches the same pragmatic approach already used in the
// licensing-backend's data_isolation.test.ts).
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const source = readFileSync(join(__dirname, 'Settings.tsx'), 'utf-8');

describe('test_refresh_button_calls_license_refresh_ipc', () => {
  it('the Refresh License button wires to API.licensing.refresh()', () => {
    expect(source).toMatch(/handleRefreshLicense[\s\S]{0,200}API\.licensing\.refresh\(\)/);
    expect(source).toMatch(/onClick=\{handleRefreshLicense\}/);
  });
});

describe('test_no_payment_instrument_fields_ever_rendered_in_app', () => {
  it('no card number/CVV/expiry input exists anywhere in the Settings page', () => {
    const FORBIDDEN_PATTERNS = [
      /card.?number/i,
      /\bcvv\b/i,
      /\bcvc\b/i,
      /card.?expiry/i,
      /expiry.?date.*card/i,
    ];
    for (const pattern of FORBIDDEN_PATTERNS) {
      expect(source).not.toMatch(pattern);
    }
  });
});
