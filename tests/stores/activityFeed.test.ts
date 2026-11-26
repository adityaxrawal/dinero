// The bell's feed is seeded from the persistent Activity log (audit F19-3).
import { describe, it, expect } from 'vitest';
import { activityToFeedItem } from '@/stores/useNotificationStore';

const entry = (over = {}) => ({
  id: 'a1',
  kind: 'scan' as const,
  outcome: 'completed',
  summary: 'Checked 10 emails: 3 booked.',
  details_json: null,
  created_at: '2026-11-27 09:30:00',
  ...over,
});

describe('activityToFeedItem', () => {
  it('maps an entry to a read feed item at its real (UTC) time', () => {
    const item = activityToFeedItem(entry());
    expect(item).toMatchObject({
      id: 'activity_a1',
      category: 'ingestion',
      severity: 'success',
      title: 'Email scan',
      message: 'Checked 10 emails: 3 booked.',
      read: true,
    });
    expect(item.timestamp).toBe(Date.parse('2026-11-27T09:30:00Z'));
  });

  it('uses the shared severity levels for failures and things needing you', () => {
    expect(activityToFeedItem(entry({ outcome: 'failed' })).severity).toBe('error');
    expect(activityToFeedItem(entry({ kind: 'ai', outcome: 'needs_review' })).severity).toBe('attention');
    expect(activityToFeedItem(entry({ kind: 'cluster', outcome: 'matched' })).title).toBe('Duplicate check');
  });
});
