/**
 * TASK-FE-005 / TASK-AUTH-001: the 5-minute loopback-listener timeout
 * (browser closed without completing, or the user never finishes) surfaces
 * to the frontend as `AppError::Auth("Authentication error: oauth_timeout")`
 * — give it a specific, actionable message rather than a generic failure
 * string. Extracted as a pure function so the mapping is unit-testable
 * without driving the real OAuth flow.
 */
export function mapOauthError(rawMessage: string | undefined | null): string {
  const msg = rawMessage || '';
  if (msg.includes('oauth_timeout')) {
    return 'Connection timed out, try again.';
  }
  return msg || 'Failed to connect Gmail. Please try again.';
}
