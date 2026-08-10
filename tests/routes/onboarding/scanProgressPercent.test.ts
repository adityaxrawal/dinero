import { describe, it, expect } from 'vitest';
import { scanProgressPercent } from '@/routes/onboarding/scanProgressPercent';

describe('scanProgressPercent', () => {
  it('returns 0 while total is still unknown (0), avoiding a divide-by-zero', () => {
    expect(scanProgressPercent(0, 0)).toBe(0);
    expect(scanProgressPercent(5, 0)).toBe(0);
  });

  it('computes a rounded percentage', () => {
    expect(scanProgressPercent(1, 3)).toBe(33);
    expect(scanProgressPercent(50, 100)).toBe(50);
    expect(scanProgressPercent(100, 100)).toBe(100);
  });
});
