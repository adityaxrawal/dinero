import { describe, it, expect } from 'vitest';
import { queryKeys } from './queryKeys';

describe('queryKeys', () => {
  it('specific keys extend their domain "all" prefix, so a broad invalidation catches them', () => {
    expect(queryKeys.transactions.list(1).slice(0, 1)).toEqual(queryKeys.transactions.all());
    expect(queryKeys.transactions.detail('t1').slice(0, 1)).toEqual(queryKeys.transactions.all());
    expect(queryKeys.dashboard.summary().slice(0, 1)).toEqual(queryKeys.dashboard.all());
    expect(queryKeys.instruments.list().slice(0, 1)).toEqual(queryKeys.instruments.all());
  });

  it('list/detail keys are stable for identical inputs (memoization-safe)', () => {
    expect(queryKeys.transactions.list(2)).toEqual(queryKeys.transactions.list(2));
    expect(queryKeys.transactions.detail('abc')).toEqual(queryKeys.transactions.detail('abc'));
  });

  it('different pages/ids produce different keys', () => {
    expect(queryKeys.transactions.list(1)).not.toEqual(queryKeys.transactions.list(2));
    expect(queryKeys.transactions.detail('a')).not.toEqual(queryKeys.transactions.detail('b'));
  });
});
