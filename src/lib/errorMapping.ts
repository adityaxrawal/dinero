/**
 * Translates backend error codes into user-facing toast content.
 *
 * Backend messages are written for developers -- they name commands, tables and
 * internal states. This module is the single place where those are turned into
 * something a user can act on, which keeps the wording consistent no matter
 * which screen triggered the failure.
 *
 * Two patterns run through the table below. Errors with a generic cause get a
 * fully written explanation and ignore the backend text entirely; errors whose
 * detail genuinely varies per occurrence fall back to `error.message` when it is
 * present, because the specific reason carries information the generic sentence
 * cannot. Where a failure has an obvious remedy, the entry also carries a route
 * and label so the toast can offer a direct way to fix it.
 */
import type { AppError } from '@/types/ipc';

/** Toast copy, plus an optional in-app action linking to the fix. */
export interface ErrorToastContent {
  title: string;
  description: string;
  actionTo?: string;
  actionLabel?: string;
}

// Keyed by AppError.code. Entries are functions rather than plain objects so
// they can incorporate the backend's message where that detail matters.
const CODE_MAP: Record<string, (error: AppError) => ErrorToastContent> = {
  LICENSE_LOCKED: () => ({
    title: 'Your subscription needs attention',
    description: 'Reactivate your license to restore full access.',
    actionTo: '/settings',
    actionLabel: 'Go to Settings',
  }),
  VALIDATION_ERROR: (error: AppError) => ({
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
  FORBIDDEN: (error: AppError) => ({
    title: 'Action not allowed',
    description: error.message || 'This action is not permitted right now.',
  }),
  NOT_FOUND: (error: AppError) => ({
    title: 'Not found',
    description: error.message || 'The item you were looking for no longer exists.',
  }),
  DATABASE_BACKUP_FAILED: (error: AppError) => ({
    title: 'Backup Failed',
    description: error.message,
  }),
  RATE_LIMITED: () => ({
    title: 'Too many requests',
    description: 'Please wait a moment and try again.',
  }),
  CONFLICT: (error: AppError) => ({
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
  PASSWORD_INCORRECT: (error: AppError) => ({
    title: 'Incorrect password',
    description: error.message || 'That password did not work for this statement.',
  }),
  FILE_TOO_LARGE: (error: AppError) => ({
    title: 'File too large',
    description: error.message || 'This file exceeds the maximum upload size.',
  }),
  INVALID_FILE_TYPE: (error: AppError) => ({
    title: 'Unsupported file type',
    description: error.message || 'Only PDF files are supported.',
  }),
};

/**
 * Resolve a structured backend error to toast content.
 *
 * Unmapped codes fall through to a generic apology that still surfaces the
 * backend message, so a newly introduced error code degrades to something
 * imperfect but informative rather than to an empty toast.
 */
export function mapAppErrorToToast(error: AppError): ErrorToastContent {
  const mapper = CODE_MAP[error.code];
  if (mapper) return mapper(error);
  return {
    title: 'Something went wrong',
    description: error.message || 'An unexpected error occurred.',
  };
}

/**
 * Best-effort message extraction from a value of unknown shape.
 *
 * Used for values that never went through the IPC error contract at all -- a
 * thrown string, a third-party library's error object -- where the only
 * available signal is a `message` property that may or may not exist.
 */
export function getErrorMessage(
  error: unknown,
  defaultMessage = 'An unexpected error occurred'
): string {
  if (
    error &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as Record<string, unknown>).message === 'string'
  ) {
    return (error as Record<string, unknown>).message as string;
  }
  return defaultMessage;
}

/**
 * The general entry point: turn anything thrown into toast content.
 *
 * Callers in catch blocks receive `unknown` and cannot know whether the value
 * came from the IPC layer or from ordinary JavaScript, so this branches on that
 * question and routes each kind to the appropriate treatment.
 */
export function getErrorToast(
  error: unknown,
  defaultMessage = 'An unexpected error occurred'
): ErrorToastContent {
  if (isAppError(error)) {
    return mapAppErrorToToast(error);
  }
  return {
    title: 'Error',
    description: getErrorMessage(error, defaultMessage),
  };
}

/**
 * Structural type guard for the IPC error contract.
 *
 * Checks for the presence of `code` and `message` rather than an instanceof,
 * because these values crossed the IPC boundary as plain deserialised JSON and
 * carry no prototype to test against.
 */
function isAppError(error: unknown): error is AppError {
  return !!error && typeof error === 'object' && 'code' in error && 'message' in error;
}
