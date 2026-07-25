// Source-scan test (see Settings.billing.test.ts precedent): AppLayout
// pulls in license state, reconciliation queries, and several IPC calls on
// mount, all unrelated to this placement change -- a full render test would
// need to mock all of it for no extra signal on this specific claim.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const source = readFileSync(join(__dirname, 'AppLayout.tsx'), 'utf-8');

describe('AppLayout scan status placement', () => {
  it('mounts ScanStatusSidebarItem in the main sidebar', () => {
    expect(source).toMatch(
      /import ScanStatusSidebarItem from '@\/components\/layout\/ScanStatusSidebarItem'/
    );
    expect(source).toMatch(/<ScanStatusSidebarItem \/>/);
  });

  it('places it inside the sidebar bottom "System & Status" block, before the floating overlay div', () => {
    const sidebarBlockIndex = source.indexOf('Bottom area (System & Status)');
    const scanStatusUsageIndex = source.indexOf('<ScanStatusSidebarItem />');
    const floatingOverlayIndex = source.indexOf('Background task indicator');

    expect(sidebarBlockIndex).toBeGreaterThan(-1);
    expect(scanStatusUsageIndex).toBeGreaterThan(sidebarBlockIndex);
    expect(scanStatusUsageIndex).toBeLessThan(floatingOverlayIndex);
  });
});
