import { describe, it, expect } from 'vitest';
import { formatDuration, estimateEtaSeconds } from '@/lib/scanTiming';

describe('formatDuration', () => {
  it('formats seconds under a minute', () => {
    expect(formatDuration(45)).toBe('45s');
  });

  it('formats minutes and seconds', () => {
    expect(formatDuration(95)).toBe('1m 35s');
  });

  it('formats hours and minutes', () => {
    expect(formatDuration(3725)).toBe('1h 2m');
  });

  it('clamps negative input to 0s', () => {
    expect(formatDuration(-5)).toBe('0s');
  });
});

describe('estimateEtaSeconds', () => {
  it('returns null when nothing has been processed yet', () => {
    expect(estimateEtaSeconds(0, 100, 10)).toBeNull();
  });

  it('returns null once processing is complete', () => {
    expect(estimateEtaSeconds(100, 100, 10)).toBeNull();
  });

  it('returns null when total is 0', () => {
    expect(estimateEtaSeconds(0, 0, 10)).toBeNull();
  });

  it('estimates remaining time from the observed rate', () => {
    // 10 processed in 20s -> 2s/item, 90 remaining -> 180s
    expect(estimateEtaSeconds(10, 100, 20)).toBe(180);
  });
});
