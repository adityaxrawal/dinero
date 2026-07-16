/** Extracts a display message from an AppError-shaped `{code, message}` throw, or any other thrown value. */
export function getErrorMessage(err: unknown, fallback = 'An unexpected error occurred.'): string {
  if (err && typeof err === 'object' && 'message' in err && typeof (err as { message: unknown }).message === 'string') {
    return (err as { message: string }).message;
  }
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  return fallback;
}
