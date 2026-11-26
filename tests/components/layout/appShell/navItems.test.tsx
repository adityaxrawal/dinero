// Statements waiting for the user get a nav badge (audit P3-L).
import { describe, it, expect } from 'vitest';
import { buildNavGroups } from '@/components/layout/appShell/navItems';

const item = (path: string, statementCount?: number) =>
  buildNavGroups(0, false, statementCount)
    .flatMap((g) => g.items)
    .find((i) => i.path === path)!;

describe('buildNavGroups', () => {
  it('badges Statements with the number waiting for the user', () => {
    expect(item('/statements', 3).badge).toBe(3);
  });

  it('shows no Statements badge when nothing is waiting', () => {
    expect(item('/statements').badge).toBe(0);
  });

  it('lists Spending limits (audit F15-1)', () => {
    expect(item('/spending-limits')).toBeDefined();
  });
});
