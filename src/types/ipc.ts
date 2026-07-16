/**
 * TASK-SETUP-013 / TASK-API-010. Shared IPC types — the canonical `AppError`
 * shape (Document 19 §3.4) previously lived as a private, unexported
 * interface inside `src/lib/ipc.ts`; moved here so `useIpcInvoke` and any
 * other consumer can share the same type instead of redefining it.
 *
 * `AppErrorCode` mirrors the exact set of string values
 * `src-tauri/src/error.rs`'s `AppError::code()` can produce -- a discriminated
 * union (not a bare `string`) so React error handling can `switch` on
 * `error.code` instead of parsing `error.message` text. Note this is a
 * many-to-one mapping, not a 1:1 mirror of the Rust enum's 10 variants:
 * `Db`/`Unknown`/`FileAccessDenied`/`Io`/`Internal` all currently produce
 * `INTERNAL_ERROR`, and `Parse`/`Validation` both produce `VALIDATION_ERROR`
 * -- `error.rs`'s own doc comment flags reconciling this against Document
 * 19 §4's full ~25-code catalog (richer, command-specific codes like
 * `SCAN_NOT_FOUND`/`CLUSTER_NOT_FOUND`) as explicitly out of this task's
 * scope; this type honestly reflects what's actually on the wire today,
 * not an aspirational richer catalog. `UNKNOWN_ERROR` is a 6th,
 * frontend-only value -- `src/lib/ipc.ts`'s `invokeCommand` catch-all for a
 * rejection that isn't even shaped like a structured `AppError` at all
 * (e.g. a raw JS/Tauri-runtime failure), never produced by Rust directly.
 */
export type AppErrorCode =
  | 'INTERNAL_ERROR'
  | 'NETWORK_ERROR'
  | 'UNAUTHORIZED'
  | 'LICENSE_LOCKED'
  | 'VALIDATION_ERROR'
  | 'UNKNOWN_ERROR'

export interface AppError {
  code: AppErrorCode
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
