// Doc 01 §10.4 "Network Communication Disclosure" — the app's exact, complete
// list of outbound network destinations. Must be presented verbatim on the
// onboarding consent screen (before Gmail authorization), in the Privacy
// Policy, and linked from Settings → Privacy → Network Activity — this
// constant is the single source of truth all three surfaces render from.
export const OUTBOUND_CHANNEL_DISCLOSURE = [
  "Gmail API — OAuth access token only; no email content is ever sent (every polling cycle)",
  "Licensing Backend — license key, device fingerprint hash, subscription status only. It never receives your transactions, balances, or any other financial data (on launch and periodic validation)",
  "Google OAuth servers — PKCE authorization code, redirect URI, scope string (one-time per Gmail sign-in)",
  "GitHub Releases — app version and OS version, for update checks (on launch and periodically)",
  "Hugging Face — a public model file download, one-time, only if you click \"Download Model\" for the optional local LLM",
  "No third-party analytics or crash-reporting services"
];

// F12 fix: Dinero's Google OAuth client is currently in Google's "Testing"
// publishing status (not yet verified) — previously these constraints were
// only documented in Beta_Onboarding_Guide.md, a file most users never open.
// Presented verbatim on the onboarding consent screen, before Gmail
// authorization, alongside the outbound-channels disclosure above.
export const BETA_PROGRAM_DISCLOSURE = [
  "This app's Google integration is in a beta \"Testing\" mode and has not completed Google's app verification — Google will show an \"unverified app\" warning during sign-in. This is expected; proceeding is safe, and no data leaves your device except as listed below.",
  "Only the first 100 Google accounts to connect can use Gmail sign-in while the app is in Testing mode.",
  "Testing-mode refresh tokens expire after 7 days of inactivity — you may need to reconnect Gmail periodically until the app completes verification.",
];
