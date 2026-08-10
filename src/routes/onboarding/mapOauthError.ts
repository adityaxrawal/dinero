/**
 * Turn a raw OAuth failure into text worth showing during onboarding.
 *
 * Only the timeout case is rewritten, because it is both the most common
 * failure -- the consent window sits open in an external browser until it
 * expires -- and the least self-explanatory in its raw form. Other messages are
 * passed through, since they usually carry a real reason from Google that is
 * more useful than any generic sentence would be.
 */
export function mapOauthError(rawMessage: string | undefined | null): string {
  const msg = rawMessage || '';
  if (msg.includes('oauth_timeout')) {
    return 'Connection timed out, try again.';
  }
  // Final fallback covers a rejection that carried no message at all.
  return msg || 'Failed to connect Gmail. Please try again.';
}
