// TASK-FE-012 / TASK-STMT-009: the dropzone's own responsibilities are the
// drag state, the three-way status line, and routing click/keyboard/drop to
// the upload hook. Validation and the upload round trip live in
// `useStatementUpload` and are mocked out here.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import StatementUploadDropzone from '@/components/statements/StatementUploadDropzone';
import { useStatementUpload } from '@/components/statements/upload/useStatementUpload';

vi.mock('@/components/statements/upload/useStatementUpload', () => ({ useStatementUpload: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const pickAndUpload = vi.fn();
const dropFiles = vi.fn();

function setUpload(over: Record<string, unknown> = {}) {
  asMock(useStatementUpload).mockReturnValue({
    isUploading: false,
    batchProgress: null,
    pickAndUpload,
    dropFiles,
    ...over,
  });
}

const dropzone = () => screen.getByTestId('dropzone');
const status = () => screen.getByRole('status');

beforeEach(() => {
  vi.clearAllMocks();
  setUpload();
});

describe('StatementUploadDropzone', () => {
  it('invites a drop when idle', () => {
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    expect(status()).toHaveTextContent('Drag and drop your PDF statements here');
  });

  it('reports upload before batch progress when both are active', () => {
    // `isUploading` is checked first: the intake round trip is still open, so
    // reporting parse progress here would understate what is left to do.
    setUpload({ isUploading: true, batchProgress: { parsed: 3, total: 10, etaSeconds: 42 } });
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    expect(status()).toHaveTextContent('Uploading…');
  });

  it('surfaces the 5-at-a-time parser cap and ETA once intake returns', () => {
    setUpload({ batchProgress: { parsed: 3, total: 10, etaSeconds: 42 } });
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    expect(status()).toHaveTextContent(
      'Parsing 3 of 10 statements (max 5 at a time)… ~42s remaining'
    );
  });

  it('highlights on drag over and clears on drag leave', () => {
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    expect(dropzone().className).not.toContain('border-primary bg-primary/10');

    fireEvent.dragOver(dropzone());
    expect(dropzone().className).toContain('border-primary');

    fireEvent.dragLeave(dropzone());
    expect(dropzone().className).not.toContain('bg-primary/10');
  });

  it('passes dropped files through and clears the drag highlight', () => {
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    fireEvent.dragOver(dropzone());

    const file = new File(['%PDF-'], 'statement.pdf', { type: 'application/pdf' });
    fireEvent.drop(dropzone(), { dataTransfer: { files: [file] } });

    expect(dropFiles).toHaveBeenCalledWith([file]);
    expect(dropzone().className).not.toContain('bg-primary/10');
  });

  it('opens the picker on click and on both activation keys', () => {
    render(<StatementUploadDropzone onUploaded={vi.fn()} />);

    fireEvent.click(dropzone());
    expect(pickAndUpload).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(dropzone(), { key: 'Enter' });
    fireEvent.keyDown(dropzone(), { key: ' ' });
    expect(pickAndUpload).toHaveBeenCalledTimes(3);

    fireEvent.keyDown(dropzone(), { key: 'Tab' });
    expect(pickAndUpload).toHaveBeenCalledTimes(3);
  });
});
