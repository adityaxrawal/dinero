/**
 * Shared error contract for the IPC boundary between the React frontend and the
 * Rust backend.
 *
 * Every command rejection is normalised into an AppError before it reaches
 * application code, so callers can branch on a stable `code` instead of
 * pattern-matching backend message text. The Rust side must emit values that
 * deserialise into this shape; the mapping from code to user-facing copy lives
 * in the error-mapping layer, not here.
 */

/**
 * Closed set of failure categories a backend command can report.
 *
 * UNKNOWN_ERROR is the catch-all applied on the frontend when a rejection does
 * not carry a recognisable structured payload at all.
 */
export type AppErrorCode =
  | 'INTERNAL_ERROR'
  | 'NETWORK_ERROR'
  | 'UNAUTHORIZED'
  | 'LICENSE_LOCKED'
  | 'VALIDATION_ERROR'
  | 'UNKNOWN_ERROR';

/**
 * A normalised backend failure.
 *
 * `message` is diagnostic text intended for logs and developers rather than
 * something to render directly, and `details` carries optional structured
 * context specific to the failing command.
 */
export interface AppError {
  code: AppErrorCode;
  message: string;
  details?: Record<string, unknown>;
}
