import { describe, it, expect } from 'vitest';
import { isGraceUrgent } from './GracePeriodBanner';

// Doc 30 TASK-BILL-004: "Day 1-3 informational amber; Day 4-7 prominent
// red." daysRemaining counts down from 7, so <=3 remaining means >=4 days
// have elapsed.
describe('isGraceUrgent', () => {
  it('is not urgent with 7/6/5/4 days remaining (Day 1-3 elapsed)', () => {
    expect(isGraceUrgent(7)).toBe(false);
    expect(isGraceUrgent(6)).toBe(false);
    expect(isGraceUrgent(5)).toBe(false);
    expect(isGraceUrgent(4)).toBe(false);
  });

  it('is urgent with 3/2/1/0 days remaining (Day 4-7 elapsed)', () => {
    expect(isGraceUrgent(3)).toBe(true);
    expect(isGraceUrgent(2)).toBe(true);
    expect(isGraceUrgent(1)).toBe(true);
    expect(isGraceUrgent(0)).toBe(true);
  });

  it('is not urgent when days remaining is unknown', () => {
    expect(isGraceUrgent(null)).toBe(false);
  });
});
