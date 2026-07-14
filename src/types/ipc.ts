/**
 * TASK-SETUP-013. Shared IPC types — the canonical `AppError` shape
 * (Document 19 §3.4) previously lived as a private, unexported interface
 * inside `src/lib/ipc.ts`; moved here so `useIpcInvoke` and any other
 * consumer can share the same type instead of redefining it.
 */
export interface AppError {
  code: string
  message: string
  details?: Record<string, unknown>
}

/**
 * Mirrors the Rust-side `ipc::responses::Payload<T>` envelope. Most
 * commands in this codebase return `T` directly and signal failure via a
 * rejected promise (Tauri's own `Err` → JS rejection mechanism), so this
 * type mainly documents that shape for any command that explicitly opts
 * into the wrapped-envelope pattern instead.
 */
export interface IpcResponse<T> {
  data: T | null
  error: string | null
}
