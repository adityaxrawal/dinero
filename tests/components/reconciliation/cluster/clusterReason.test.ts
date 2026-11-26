// Why a cluster was formed, shown in plain words (audit F10-7).
import { describe, it, expect } from 'vitest';
import { clusterReasonText } from '@/lib/clusterDiff';

describe('clusterReasonText', () => {
  it('explains both reasons the engine writes', () => {
    expect(clusterReasonText('multiple_high_score_candidates')).toMatch(/more than one/);
    expect(clusterReasonText('mid_range_score')).toMatch(/partial match/);
  });

  it('shows nothing for an unknown or missing reason', () => {
    expect(clusterReasonText('something_else')).toBeNull();
    expect(clusterReasonText(null)).toBeNull();
  });
});
