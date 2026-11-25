// The picker path checks the type client-side and sends paths; Rust reads the
// files and enforces the size limit (audit N-D1). The drop path can't read
// absolute paths, so it hands back to the picker.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useStatementUpload } from '@/components/statements/upload/useStatementUpload';
import { API } from '@/lib/ipc';

const toast = vi.fn();
const watchDraftOrigin = vi.fn();
const setBatchProgress = vi.fn();
const onUploaded = vi.fn();
const open = vi.fn();

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => open(...a) }));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/errorMapping', () => ({
  getErrorMessage: (e: unknown) => String(e),
  getErrorToast: () => ({ title: 'failed' }),
}));
vi.mock('@/lib/GlobalStateContext', () => ({
  useGlobalState: () => ({ batchProgress: null, setBatchProgress, watchDraftOrigin }),
}));
vi.mock('@/lib/ipc', () => ({ API: { statements: { upload: vi.fn() } } }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.statements.upload).mockResolvedValue([
    { status: 'queued', statement_id: 's1', filename: 'a.pdf' },
  ]);
});

const setup = () => renderHook(() => useStatementUpload(onUploaded));

describe('useStatementUpload picker path', () => {
  it('does nothing when the picker is dismissed', async () => {
    open.mockResolvedValue(null);
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(API.statements.upload).not.toHaveBeenCalled();
  });

  it('uploads a single selection as a one-item batch', async () => {
    open.mockResolvedValue('/tmp/a.pdf');
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(API.statements.upload).toHaveBeenCalledWith(['/tmp/a.pdf']);
    expect(watchDraftOrigin).toHaveBeenCalledWith('s1');
    expect(onUploaded).toHaveBeenCalled();
  });

  it('rejects a typed non-PDF path before uploading anything', async () => {
    open.mockResolvedValue(['/tmp/notes.txt']);
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(API.statements.upload).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ description: 'Only PDF files are allowed.' })
    );
  });

  it('sends paths and reports the size error Rust returns for an oversized file', async () => {
    open.mockResolvedValue(['/tmp/big.pdf', '/tmp/ok.pdf']);
    asMock(API.statements.upload).mockResolvedValue([
      {
        status: 'error: file_too_large: file must not exceed 25 MB',
        statement_id: '',
        filename: 'big.pdf',
      },
      { status: 'queued', statement_id: 's2', filename: 'ok.pdf' },
    ]);
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(API.statements.upload).toHaveBeenCalledWith(['/tmp/big.pdf', '/tmp/ok.pdf']);
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ description: expect.stringContaining('file_too_large') })
    );
    expect(watchDraftOrigin).toHaveBeenCalledWith('s2');
  });

  it('reports an upload round-trip failure rather than throwing', async () => {
    open.mockResolvedValue(['/tmp/a.pdf']);
    asMock(API.statements.upload).mockRejectedValue(new Error('disk full'));
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Some Uploads Failed' }));
    expect(onUploaded).toHaveBeenCalled();
  });

  it('reports a picker failure through the error toast', async () => {
    open.mockRejectedValue(new Error('no dialog'));
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'failed' }));
  });
});

describe('useStatementUpload drop path', () => {
  it('rejects a dropped non-PDF without opening the picker', async () => {
    const { result } = setup();
    await act(async () => {
      result.current.dropFiles([{ name: 'a.txt', size: 10, type: 'text/plain' } as File]);
    });
    expect(open).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ description: 'Only PDF files are allowed.' })
    );
  });

  it('hands a valid drop back to the picker, which does carry real paths', async () => {
    open.mockResolvedValue(['/tmp/a.pdf']);
    const { result } = setup();
    await act(async () => {
      result.current.dropFiles([{ name: 'a.pdf', size: 10, type: 'application/pdf' } as File]);
    });
    expect(open).toHaveBeenCalled();
  });
});
