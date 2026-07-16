import { describe, it, expect } from 'vitest';
import { classifyBillUrgency } from './classifyBillUrgency';

const now = new Date('2026-07-16T00:00:00Z');

describe('classifyBillUrgency', () => {
  it('classifies a past due date as overdue', () => {
    expect(classifyBillUrgency('2026-07-10', now)).toBe('overdue');
  });

  it('classifies due within 3 days as critical', () => {
    expect(classifyBillUrgency('2026-07-16', now)).toBe('critical');
    expect(classifyBillUrgency('2026-07-19', now)).toBe('critical');
  });

  it('classifies due within 4-7 days as warning', () => {
    expect(classifyBillUrgency('2026-07-20', now)).toBe('warning');
    expect(classifyBillUrgency('2026-07-23', now)).toBe('warning');
  });

  it('classifies due beyond 7 days as normal', () => {
    expect(classifyBillUrgency('2026-08-01', now)).toBe('normal');
  });
});
