// Source-scan test: AppLayout mounts SidebarNotificationCenter in the main sidebar
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const source = readFileSync(join(__dirname, '../../../src/components/layout/AppLayout.tsx'), 'utf-8');

describe('AppLayout notification center placement', () => {
  it('mounts SidebarNotificationCenter in the main sidebar', () => {
    expect(source).toMatch(
      /import SidebarNotificationCenter from '@\/components\/layout\/SidebarNotificationCenter'/
    );
    expect(source).toMatch(/<SidebarNotificationCenter \/>/);
  });

  it('places it inside the sidebar bottom "System & Status" block', () => {
    const sidebarBlockIndex = source.indexOf('Bottom area (System & Status)');
    const notificationCenterIndex = source.indexOf('<SidebarNotificationCenter />');

    expect(sidebarBlockIndex).toBeGreaterThan(-1);
    expect(notificationCenterIndex).toBeGreaterThan(sidebarBlockIndex);
  });
});

/**
 * `SidebarNotificationCenter` is the single owner of scan and background-task
 * status: `useNotificationStore` turns `scan_progress` into a live task card
 * and handles `background_task_progress` directly. The older
 * `ScanStatusSidebarItem` / `BackgroundTaskIndicator` components rendered the
 * same two signals from separate state and were deleted rather than left
 * duplicating it.
 */
describe('AppLayout has no duplicate scan/background-task surface', () => {
  it('does not re-introduce the superseded indicator components', () => {
    expect(source).not.toMatch(/ScanStatusSidebarItem/);
    expect(source).not.toMatch(/BackgroundTaskIndicator/);
  });
});
