// TASK-FE-012 / TASK-STMT-009: the dropzone's own responsibilities are the
// drag state, the three-way status line, and routing click/keyboard/drop to
// the upload hook. Drops arrive through Tauri's native drag-drop event, which
// carries real paths (audit F8-3). Validation and the upload round trip live in
// `useStatementUpload` and are mocked out here.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import StatementUploadDropzone from '@/components/statements/StatementUploadDropzone';
import { useStatementUpload } from '@/components/statements/upload/useStatementUpload';

type DragPayload =
  | { type: 'enter' | 'drop'; paths: string[]; position: { x: number; y: number } }
  | { type: 'over'; position: { x: number; y: number } }
  | { type: 'leave' };
let dragHandler: ((e: { payload: DragPayload }) => void) | null = null;
const unlisten = vi.fn();
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (h: typeof dragHandler) => {
      dragHandler = h;
      return unlisten;
    },
  }),
}));

vi.mock('@/components/statements/upload/useStatementUpload', () => ({
  useStatementUpload: vi.fn(),
}));

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
  dragHandler = null;
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

  // The zone occupies (0,0)-(100,100) in CSS pixels; the event is physical.
  async function renderWithZone() {
    const view = render(<StatementUploadDropzone onUploaded={vi.fn()} />);
    dropzone().getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 100, bottom: 100 }) as DOMRect;
    await waitFor(() => expect(dragHandler).not.toBeNull());
    return view;
  }
  const fire = (payload: DragPayload) => act(() => dragHandler!({ payload }));

  it('highlights while a drag is over the zone and clears on leave', async () => {
    await renderWithZone();
    fire({ type: 'over', position: { x: 50, y: 50 } });
    expect(dropzone().className).toContain('bg-primary/10');
    fire({ type: 'over', position: { x: 500, y: 500 } });
    expect(dropzone().className).not.toContain('bg-primary/10');
    fire({ type: 'over', position: { x: 50, y: 50 } });
    fire({ type: 'leave' });
    expect(dropzone().className).not.toContain('bg-primary/10');
  });

  it('uploads the dropped paths when dropped on the zone', async () => {
    await renderWithZone();
    fire({ type: 'over', position: { x: 50, y: 50 } });
    fire({ type: 'drop', paths: ['/tmp/a.pdf'], position: { x: 50, y: 50 } });
    expect(dropFiles).toHaveBeenCalledWith(['/tmp/a.pdf']);
    expect(pickAndUpload).not.toHaveBeenCalled();
    expect(dropzone().className).not.toContain('bg-primary/10');
  });

  it('ignores a drop elsewhere in the window', async () => {
    await renderWithZone();
    fire({ type: 'drop', paths: ['/tmp/a.pdf'], position: { x: 500, y: 500 } });
    expect(dropFiles).not.toHaveBeenCalled();
  });

  it('stops listening on unmount', async () => {
    const { unmount } = await renderWithZone();
    unmount();
    expect(unlisten).toHaveBeenCalled();
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
