// The upload round trip reports per-file status strings, not exceptions; these
// pin how those get sorted into the four things a user needs told apart.
import { describe, it, expect } from 'vitest';
import { classifyUploadResults, uploadToasts, emptyOutcome } from '@/components/statements/upload/uploadOutcome';
import { validateDroppedFiles, isPdf, basename, MAX_FILE_SIZE_BYTES } from '@/components/statements/upload/validation';

const file = (name: string, size: number, type = 'application/pdf') =>
  ({ name, size, type }) as File;

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
      { status: 'error: File access denied', filename: 'a.pdf' },
      { status: 'error: duplicate statement', filename: 'b.pdf' },
      { status: 'error: parser exploded', filename: 'c.pdf' },
    ] as never);
    expect(outcome.accessDenied).toBe(true);
    expect(outcome.duplicates).toEqual(['b.pdf']);
    expect(outcome.otherFailures).toEqual(['parser exploded']);
    expect(outcome.succeeded).toBe(0);
  });

  it('also treats "Permission denied" as the same macOS block', () => {
    expect(
      classifyUploadResults([{ status: 'error: Permission denied', filename: 'a.pdf' }] as never)
        .accessDenied
    ).toBe(true);
  });

  it('names an unnamed duplicate rather than showing "undefined"', () => {
    const outcome = classifyUploadResults([{ status: 'error: duplicate' }] as never);
    expect(outcome.duplicates).toEqual(['A file']);
  });

  it('only watches drafts for uploads that actually queued', () => {
    const outcome = classifyUploadResults([
      { status: 'error: duplicate', statement_id: 's1' },
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
  it('accepts a PDF by extension or by MIME type', () => {
    expect(isPdf('statement.PDF')).toBe(true);
    expect(isPdf('statement', 'application/pdf')).toBe(true);
    expect(isPdf('statement.txt', 'text/plain')).toBe(false);
  });

  it('rejects a dropped non-PDF before anything is uploaded', () => {
    const error = validateDroppedFiles([file('a.pdf', 10), file('notes.txt', 10, 'text/plain')]);
    expect(error?.description).toMatch(/Only PDF files/);
  });

  it('rejects a file past the 25MB client-side guard, naming it', () => {
    const error = validateDroppedFiles([file('huge.pdf', MAX_FILE_SIZE_BYTES + 1)]);
    expect(error?.title).toBe('File Too Large');
    expect(error?.description).toContain('huge.pdf');
  });

  it('passes a valid batch through', () => {
    expect(validateDroppedFiles([file('a.pdf', 100), file('b.pdf', 200)])).toBeNull();
  });

  it('takes the last path segment on either separator', () => {
    expect(basename('/Users/x/a.pdf')).toBe('a.pdf');
    expect(basename('C:\\docs\\b.pdf')).toBe('b.pdf');
    expect(basename('c.pdf')).toBe('c.pdf');
  });
});
