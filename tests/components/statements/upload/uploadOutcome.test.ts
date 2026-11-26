// The upload round trip reports per-file status strings, not exceptions; these
// pin how those get sorted into the four things a user needs told apart.
import { describe, it, expect } from 'vitest';
import {
  classifyUploadResults,
  uploadToasts,
  emptyOutcome,
} from '@/components/statements/upload/uploadOutcome';
import { isPdf } from '@/components/statements/upload/validation';

describe('classifyUploadResults', () => {
  it('counts a clean batch as all-succeeded', () => {
    const outcome = classifyUploadResults([
      { status: 'queued', statement_id: 's1', filename: 'a.pdf' },
      { status: 'queued', statement_id: 's2', filename: 'b.pdf' },
    ] as never);
    expect(outcome.succeeded).toBe(2);
    expect(outcome.queuedIds).toEqual(['s1', 's2']);
    expect(outcome.duplicates).toEqual([]);
  });

  it('separates a TCC block from a duplicate from an unknown failure', () => {
    const outcome = classifyUploadResults([
      { status: 'error: File access denied', filename: 'a.pdf', error_code: 'access_denied' },
      { status: 'error: duplicate statement', filename: 'b.pdf', error_code: 'duplicate' },
      { status: 'error: parser exploded', filename: 'c.pdf', error_code: 'failed' },
    ] as never);
    expect(outcome.accessDenied).toBe(true);
    expect(outcome.duplicates).toEqual(['b.pdf']);
    expect(outcome.otherFailures).toEqual(['parser exploded']);
    expect(outcome.succeeded).toBe(0);
  });

  it('classifies on the error code, never the English text (audit F8-7)', () => {
    const outcome = classifyUploadResults([
      { status: 'error: duplicate in the text only', filename: 'a.pdf', error_code: 'failed' },
    ] as never);
    expect(outcome.duplicates).toEqual([]);
    expect(outcome.otherFailures).toEqual(['duplicate in the text only']);
  });

  it('names an unnamed duplicate rather than showing "undefined"', () => {
    const outcome = classifyUploadResults([{ status: 'error: duplicate', error_code: 'duplicate' }] as never);
    expect(outcome.duplicates).toEqual(['A file']);
  });

  it('only watches drafts for uploads that actually queued', () => {
    const outcome = classifyUploadResults([
      { status: 'error: duplicate', statement_id: 's1', error_code: 'duplicate' },
      { status: 'queued' },
    ] as never);
    expect(outcome.queuedIds).toEqual([]);
  });
});

describe('uploadToasts', () => {
  it('says nothing when nothing happened', () => {
    expect(uploadToasts(emptyOutcome(), 0)).toEqual([]);
  });

  it('reports every distinct outcome of a mixed batch', () => {
    const outcome = {
      ...emptyOutcome(),
      accessDenied: true,
      succeeded: 2,
      duplicates: ['b.pdf'],
      otherFailures: ['boom'],
    };
    const titles = uploadToasts(outcome, 4).map((t) => t.title);
    expect(titles).toEqual([
      'File Access Denied',
      '2 of 4 Uploads Started',
      'Already Uploaded',
      'Some Uploads Failed',
    ]);
  });

  it('drops the "n of m" wording for a single upload', () => {
    const [spec] = uploadToasts({ ...emptyOutcome(), succeeded: 1 }, 1);
    expect(spec.title).toBe('Upload Started');
  });

  it('caps the names it lists so one bad batch cannot flood the toast', () => {
    const outcome = { ...emptyOutcome(), duplicates: ['one', 'two', 'three', 'four', 'five'] };
    const { description } = uploadToasts(outcome, 5)[0];
    expect(description).toContain('one, two, three —');
    expect(description).not.toContain('four');
  });
});

describe('validation', () => {
  it('accepts a PDF by extension, case-insensitively', () => {
    expect(isPdf('/tmp/statement.PDF')).toBe(true);
    expect(isPdf('/tmp/statement.txt')).toBe(false);
  });
});
