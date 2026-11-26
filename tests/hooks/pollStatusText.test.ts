// Real "last checked / next check" text from the poller (audit F5-6).
import { describe, it, expect } from 'vitest';
import { pollStatusText } from '@/hooks/queries/usePollStatus';

const now = new Date('2026-11-26T10:00:00Z');

describe('pollStatusText', () => {
  it('reports the last check and the next one', () => {
    expect(
      pollStatusText(
        {
          last_checked_at: '2026-11-26T09:58:00Z',
          next_check_at: '2026-11-26T10:02:00Z',
          last_check_failed: false,
          paused: false,
        },
        now
      )
    ).toBe('Last checked 2 mins ago · next check in about 2 min');
  });

  it('says when nothing has been checked yet, a check failed, or checks are paused', () => {
    expect(
      pollStatusText({ last_checked_at: null, next_check_at: null, last_check_failed: true, paused: false }, now)
    ).toBe('Not checked yet this session · last check failed');
    expect(
      pollStatusText({ last_checked_at: null, next_check_at: null, last_check_failed: false, paused: true }, now)
    ).toBe('Background checks paused');
  });
});
