import { AlertCircle, ShieldCheck } from 'lucide-react';
import { BETA_PROGRAM_DISCLOSURE, OUTBOUND_CHANNEL_DISCLOSURE } from '@/constants/privacy';

interface GmailConsentScreenProps {
  loading: boolean;
  oauthError: string | null;
  showSkip: boolean;
  onSkip: () => void;
}

/**
 * TASK-FE-005 (Doc 30): renders the full Gmail consent text (scopes, beta
 * "Testing" mode constraints, outbound-channels disclosure — Document 01
 * §10.4/TASK-AUTH-002's exact wording via the shared privacy constants).
 * Purely presentational — the parent (`Onboarding.tsx`'s step wizard) still
 * owns the "I Understand, Continue to Google" submit action in its shared
 * `CardFooter` (same placement as every other step's Continue button) and
 * the `auth_google_start` call / loading / error state driving `loading`
 * and `oauthError` here. `oauthError` distinguishes the 5-minute
 * loopback-listener timeout (TASK-AUTH-001) from any other OAuth failure —
 * that distinction is made by the parent's catch block, this component just
 * displays whatever message it's given.
 */
export default function GmailConsentScreen({
  loading,
  oauthError,
  showSkip,
  onSkip,
}: GmailConsentScreenProps) {
  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-4 text-center">
      <div className="mx-auto w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
        <ShieldCheck className="w-6 h-6 text-primary" aria-hidden="true" />
      </div>
      <div>
        <h3 className="text-lg font-medium">Connect your Gmail</h3>
        <p className="text-sm text-muted-foreground mt-2">
          <span>We require read-only access to parse financial emails.</span>
          Your credentials are never stored.
        </p>
      </div>
      <div className="bg-secondary/50 p-4 rounded-md text-left" aria-label="Requested Gmail scopes">
        <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Requested Scopes</span>
        <ul className="mt-2 text-sm space-y-1">
          <li>
            •{' '}
            <code className="text-xs bg-muted px-1 rounded">
              https://www.googleapis.com/auth/gmail.readonly
            </code>
          </li>
        </ul>
      </div>

      {/* F12 fix: beta/"Testing" OAuth mode constraints, surfaced before
          Gmail authorization rather than only in a separate onboarding
          guide doc. */}
      <div
        className="bg-amber-500/10 border border-amber-500/30 p-4 rounded-md text-left"
        aria-label="Beta program disclosure"
      >
        <span className="text-xs font-semibold uppercase tracking-wider text-amber-700">
          Beta Program — Google Sign-In Limitations
        </span>
        <ul className="mt-2 text-xs space-y-1.5 text-muted-foreground" style={{ listStyleType: 'disc', paddingLeft: '16px' }}>
          {BETA_PROGRAM_DISCLOSURE.map((item, i) => (
            <li key={i}>{item}</li>
          ))}
        </ul>
      </div>

      {/* Doc 01 §10.4: presented verbatim on the onboarding consent screen,
          before Gmail authorization. */}
      <div className="bg-secondary/50 p-4 rounded-md text-left" aria-label="Outbound network channels disclosure">
        <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Outbound Channels Disclosure
        </span>
        <ul className="mt-2 text-xs space-y-1 text-muted-foreground" style={{ listStyleType: 'disc', paddingLeft: '16px' }}>
          {OUTBOUND_CHANNEL_DISCLOSURE.map((item, i) => (
            <li key={i}>{item}</li>
          ))}
        </ul>
      </div>

      {oauthError && (
        <div
          role="alert"
          className="flex items-center gap-2 text-red-700 bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2 text-sm"
        >
          <AlertCircle className="w-4 h-4 shrink-0" aria-hidden="true" />
          {oauthError}
        </div>
      )}

      {/* G2 fix: statement-only users previously had no way to finish
          onboarding without connecting Gmail. */}
      {showSkip && (
        <button
          type="button"
          onClick={onSkip}
          disabled={loading}
          className="text-sm text-muted-foreground underline underline-offset-2 hover:text-foreground disabled:opacity-50"
        >
          Skip — I'll upload statements manually
        </button>
      )}
    </div>
  );
}
