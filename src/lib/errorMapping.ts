import type { AppError, AppErrorCode } from '@/types/ipc';

/**
 * TASK-FE-018 (Doc 30): "maps every AppError variant to a specific toast
 * message/icon (LicenseLocked → 'Your subscription needs attention' with a
 * Settings link; Validation → the specific field-level message; Network →
 * 'Check your internet connection')." The single source of truth for
 * turning a raw `AppError` (Document 19 §3.4, `src/types/ipc.ts`) into
 * user-facing toast copy, so new call sites don't each invent their own
 * wording for the same backend error code.
 */
export interface ErrorToastContent {
  title: string;
  description: string;
  /** Hash-router path for a clickable toast action, e.g. '/settings'. */
  actionTo?: string;
  actionLabel?: string;
}

const CODE_MAP: Partial<Record<AppErrorCode, (error: AppError) => ErrorToastContent>> = {
  LICENSE_LOCKED: () => ({
    title: 'Your subscription needs attention',
    description: 'Reactivate your license to restore full access.',
    actionTo: '/settings',
    actionLabel: 'Go to Settings',
  }),
  LICENSE_INVALID: () => ({
    title: 'Your subscription needs attention',
    description: 'Your license could not be validated. Reactivate to restore full access.',
    actionTo: '/settings',
    actionLabel: 'Go to Settings',
  }),
  // Validation/Parse both map to VALIDATION_ERROR (src/types/ipc.ts) --
  // the backend's message is already the specific field-level detail, so
  // it's surfaced verbatim rather than replaced with generic copy.
  VALIDATION_ERROR: (error) => ({
    title: 'Check your input',
    description: error.message || 'One of the fields you entered is invalid.',
  }),
  NETWORK_ERROR: () => ({
    title: 'Connection problem',
    description: 'Check your internet connection and try again.',
  }),
  UNAUTHORIZED: () => ({
    title: 'Not authorized',
    description: 'You need to reconnect your account to continue.',
    actionTo: '/settings',
    actionLabel: 'Go to Settings',
  }),
  FORBIDDEN: (error) => ({
    title: 'Action not allowed',
    description: error.message || 'This action is not permitted right now.',
  }),
  NOT_FOUND: (error) => ({
    title: 'Not found',
    description: error.message || 'The item you were looking for no longer exists.',
  }),
  RATE_LIMITED: () => ({
    title: 'Too many requests',
    description: 'Please wait a moment and try again.',
  }),
  CONFLICT: (error) => ({
    title: 'Conflict',
    description: error.message || 'This conflicts with existing data.',
  }),
  GMAIL_NOT_CONNECTED: () => ({
    title: 'Gmail not connected',
    description: 'Connect a Gmail account in Settings to use this feature.',
    actionTo: '/settings',
    actionLabel: 'Go to Settings',
  }),
  GMAIL_API_ERROR: () => ({
    title: 'Gmail sync problem',
    description: 'Something went wrong talking to Gmail. This usually resolves on its own.',
  }),
  KEYCHAIN_ACCESS_DENIED: () => ({
    title: 'Keychain access needed',
    description: 'Dinero needs Keychain access to protect your data. Check System Settings.',
  }),
  PASSWORD_INCORRECT: (error) => ({
    title: 'Incorrect password',
    description: error.message || 'That password did not work for this statement.',
  }),
  FILE_TOO_LARGE: (error) => ({
    title: 'File too large',
    description: error.message || 'This file exceeds the maximum upload size.',
  }),
  INVALID_FILE_TYPE: (error) => ({
    title: 'Unsupported file type',
    description: error.message || 'Only PDF files are supported.',
  }),
};

export function mapAppErrorToToast(error: AppError): ErrorToastContent {
  const mapper = CODE_MAP[error.code];
  if (mapper) return mapper(error);
  return {
    title: 'Something went wrong',
    description: error.message || 'An unexpected error occurred.',
  };
}
