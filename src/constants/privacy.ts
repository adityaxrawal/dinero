/**
 * User-facing disclosures of every network destination this app contacts.
 *
 * This is the single source of truth behind the privacy screens, and it exists
 * because the app's core promise is that financial data stays on the device.
 * Each entry names a destination, exactly what is transmitted to it, and how
 * often -- phrased so that what is *not* sent is as clear as what is.
 *
 * Treat this as a contract rather than as copy: adding an outbound request
 * anywhere in the codebase means adding it here too, otherwise the disclosure
 * silently becomes untrue.
 */

/** Prose bullet form, used where the disclosure is read as a list. */
export const OUTBOUND_CHANNEL_DISCLOSURE = [
  'Gmail API — OAuth access token only; no email content is ever sent (every polling cycle)',
  'Licensing Backend — license key, device fingerprint hash, subscription status only. It never receives your transactions, balances, or any other financial data (on launch and periodic validation)',
  'Google OAuth servers — PKCE authorization code, redirect URI, scope string (one-time per Gmail sign-in)',
  'GitHub Releases — app version and OS version, for update checks (on launch and periodically)',
  'Hugging Face — a public model file download, one-time, only if you click "Download Model" for the optional local LLM',
  'No third-party analytics or crash-reporting services',
];

/** One row of the tabular disclosure: where, what, and how often. */
export interface NetworkDisclosureRow {
  destination: string;
  dataSent: string;
  when: string;
}

/**
 * The same disclosures in structured form, for rendering as a table.
 *
 * Kept alongside the prose list above rather than derived from it, since the two
 * are worded for different presentations. They must be updated together.
 */
export const NETWORK_DISCLOSURE_TABLE: NetworkDisclosureRow[] = [
  {
    destination: 'Gmail API',
    dataSent: 'OAuth access token only — no email body content is ever sent',
    when: 'On every polling cycle',
  },
  {
    destination: 'Licensing Backend',
    dataSent: 'License key, device fingerprint hash, subscription status',
    when: 'On app launch and periodic validation',
  },
  {
    destination: 'Google OAuth servers',
    dataSent: 'PKCE authorization code, redirect URI, scope string',
    when: 'One-time per Gmail sign-in session',
  },
  {
    destination: 'GitHub Releases API',
    dataSent: 'App version string, OS version string',
    when: 'On app launch and periodically',
  },
  {
    destination: 'Hugging Face',
    dataSent:
      'None sent — a unidirectional, unauthenticated HTTP GET for a public .gguf model file',
    when: 'One-time, only if you click "Download Model" for the optional local LLM',
  },
];

/**
 * Consequences of the Google integration still being in unverified Testing mode.
 *
 * Shown before Gmail sign-in so the "unverified app" warning, the 100-account
 * cap, and the 7-day refresh-token expiry are expected rather than alarming.
 * These entries become removable once Google verification completes.
 */
export const BETA_PROGRAM_DISCLOSURE = [
  'This app\'s Google integration is in a beta "Testing" mode and has not completed Google\'s app verification — Google will show an "unverified app" warning during sign-in. This is expected; proceeding is safe, and no data leaves your device except as listed below.',
  'Only the first 100 Google accounts to connect can use Gmail sign-in while the app is in Testing mode.',
  'Testing-mode refresh tokens expire after 7 days of inactivity — you may need to reconnect Gmail periodically until the app completes verification.',
];
