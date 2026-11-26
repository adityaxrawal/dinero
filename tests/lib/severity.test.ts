// One set of severity levels across channels (audit Phase 2 2.1).
import { describe, it, expect } from 'vitest';
import { fromSystemWarning } from '@/lib/severity';

describe('fromSystemWarning', () => {
  it.each([
    ['critical', 'error'],
    ['degraded', 'attention'],
    ['info', 'info'],
  ] as const)('maps %s to %s', (level, expected) => {
    expect(fromSystemWarning(level)).toBe(expected);
  });
});
