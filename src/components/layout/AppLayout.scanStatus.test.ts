// Source-scan test: AppLayout mounts SidebarNotificationCenter in the main sidebar
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const source = readFileSync(join(__dirname, 'AppLayout.tsx'), 'utf-8');

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
