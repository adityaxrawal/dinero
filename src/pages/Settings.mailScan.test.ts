// Source-scan test, matching Settings.billing.test.ts's precedent: a full
// render of Settings.tsx requires mocking every tab's unrelated IPC calls
// on mount for no extra signal on this feature's specific claims.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const source = readFileSync(join(__dirname, 'Settings.tsx'), 'utf-8');

describe('Mail Scan: cancel button', () => {
  it('opens an in-app confirm dialog rather than a native OS dialog', () => {
    // A native `ask()`/`window.confirm()` dialog renders outside React's
    // tree and was found to overlap/garble the button in the Tauri
    // webview -- this codebase already has a real Dialog component
    // (see DeleteAccountSection.tsx) for exactly this kind of
    // confirm-before-destructive-action flow, so cancel reuses it instead.
    // (Other, unrelated flows in this file -- license deactivation,
    // recovery phrase -- still legitimately use the native dialog; this
    // only asserts the cancel-scan handler itself doesn't.)
    expect(source).toMatch(/onClick=\{handleCancelClick\}/);
    expect(source).toMatch(/const handleCancelClick = \(\) => setCancelDialogOpen\(true\)/);
    expect(source).toMatch(/<Dialog open=\{cancelDialogOpen\} onOpenChange=\{setCancelDialogOpen\}>/);
  });

  it('only cancels after the dialog is confirmed, then calls handleCancelScan', () => {
    expect(source).toMatch(/onClick=\{handleConfirmCancelScan\}/);
    expect(source).toMatch(
      /const handleConfirmCancelScan = async \(\) => \{[\s\S]{0,120}await handleCancelScan\(\)/
    );
  });

  it('is only rendered while a scan is running', () => {
    expect(source).toMatch(
      /\{scanStatus === 'running' && \(\s*<Button[\s\S]{0,120}onClick=\{handleCancelClick\}/
    );
  });
});

describe('Mail Scan: cancelled state', () => {
  it('shows a distinct cancelled message, not reusing done/error copy', () => {
    expect(source).toMatch(/'Scan cancelled\.'/);
  });

  it('the Clear button also resets after a cancelled scan', () => {
    expect(source).toMatch(
      /scanStatus === 'done' \|\|\s*scanStatus === 'error' \|\|\s*scanStatus === 'cancelled'/
    );
    expect(source).toMatch(/onClick=\{resetScan\}/);
  });
});

describe('Mail Scan: elapsed time + ETA', () => {
  it('renders live elapsed time and ETA while running, using the shared helpers', () => {
    expect(source).toMatch(
      /import \{ formatDuration, estimateEtaSeconds \} from '@\/lib\/scanTiming'/
    );
    expect(source).toMatch(/import \{ useNowTicker \} from '@\/hooks\/useNowTicker'/);
    expect(source).toMatch(/estimateEtaSeconds\(/);
  });

  it('shows a frozen final duration once the scan finishes, worded differently for success vs. cancel/error', () => {
    expect(source).toMatch(/Completed in \$\{formatDuration\(elapsedSeconds\)\}/);
    expect(source).toMatch(/Ran for \$\{formatDuration\(elapsedSeconds\)\}/);
  });
});
