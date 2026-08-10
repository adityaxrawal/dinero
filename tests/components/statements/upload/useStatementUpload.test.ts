// The picker path validates client-side before any upload is attempted; the
// drop path can't read absolute paths, so it hands back to the picker.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useStatementUpload } from '@/components/statements/upload/useStatementUpload';
import { API } from '@/lib/ipc';
import { MAX_FILE_SIZE_BYTES } from '@/components/statements/upload/validation';

const toast = vi.fn();
const watchDraftOrigin = vi.fn();
const setBatchProgress = vi.fn();
const onUploaded = vi.fn();
const open = vi.fn();
const readFile = vi.fn();

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => open(...a) }));
vi.mock('@tauri-apps/plugin-fs', () => ({ readFile: (...a: unknown[]) => readFile(...a) }));
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
const bytes = (n: number) => ({ byteLength: n });

beforeEach(() => {
  vi.clearAllMocks();
  readFile.mockResolvedValue(bytes(1000));
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

  it('rejects a typed non-PDF path before reading a single byte', async () => {
    open.mockResolvedValue(['/tmp/notes.txt']);
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(readFile).not.toHaveBeenCalled();
    expect(API.statements.upload).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ description: 'Only PDF files are allowed.' })
    );
  });

  it('names every oversized file and uploads none of them', async () => {
    open.mockResolvedValue(['/tmp/big.pdf', '/tmp/ok.pdf']);
    readFile
      .mockResolvedValueOnce(bytes(MAX_FILE_SIZE_BYTES + 1))
      .mockResolvedValueOnce(bytes(10));
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(API.statements.upload).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ description: expect.stringContaining('big.pdf') })
    );
  });

  it('reports an upload round-trip failure rather than throwing', async () => {
    open.mockResolvedValue(['/tmp/a.pdf']);
    asMock(API.statements.upload).mockRejectedValue(new Error('disk full'));
    const { result } = setup();
    await act(async () => {
      await result.current.pickAndUpload();
    });
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Some Uploads Failed' })
    );
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
      result.current.dropFiles([
        { name: 'a.pdf', size: 10, type: 'application/pdf' } as File,
      ]);
    });
    expect(open).toHaveBeenCalled();
  });
});
