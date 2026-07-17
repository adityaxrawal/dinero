import { useState, useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { FileUp, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useGlobalState } from '@/lib/GlobalStateContext';

/**
 * TASK-FE-017 (Doc 30): "the contingency UI for the scenario where Google
 * CASA verification is delayed/fails or gmail.readonly access is
 * revoked/restricted (Document 01's BR-01 risk) ... directing the user to
 * the Statement Upload flow as the primary ingestion path in the interim,
 * without blocking any other part of the app."
 *
 * Doc 30 describes detecting this via "a specific lock-reason flag from
 * the backend" — TASK-AUTH-016 (this task's own dependency), which this
 * flag would come from, is a documentation/compliance-readiness task with
 * no code deliverable at all ("Acceptance criteria: none ... not
 * unit-testable" — its own text). No such flag exists anywhere in the
 * backend. The real, already-available signal for "Gmail isn't a working
 * ingestion path right now" is `connectedAccounts` from
 * `GlobalStateContext` (backed by `settings_get_connected_accounts`,
 * TASK-FE-015's real per-account status): zero accounts means either the
 * user chose statement-only at onboarding (`GmailConsentScreen`'s "Skip —
 * I'll upload statements manually") or every previously-connected account
 * has since fully disconnected — CASA failure/revocation and plain user
 * choice are indistinguishable from here, but the correct UI response
 * ("Gmail sync isn't available, use Statement Upload") is identical either
 * way, so no fabricated reason is invented.
 *
 * Dismissable-but-recurring (same pattern as `GracePeriodBanner`): local
 * component state, not persisted, so it reappears on the next mount rather
 * than being permanently suppressed by one click. Suppressed on the
 * Statements route itself — telling a user already on the upload page to
 * go upload statements is just noise.
 */
export default function StatementOnlyModeBanner() {
  const { connectedAccounts } = useGlobalState();
  const location = useLocation();
  const navigate = useNavigate();
  const [dismissed, setDismissed] = useState(false);

  const hasNoGmailAccount = connectedAccounts.length === 0;

  useEffect(() => {
    if (!hasNoGmailAccount) setDismissed(false);
  }, [hasNoGmailAccount]);

  if (!hasNoGmailAccount || dismissed || location.pathname === '/statements') return null;

  return (
    <div
      role="status"
      className="flex items-center gap-3 px-4 py-3 mb-4 rounded-lg border border-border bg-secondary/40 text-sm"
    >
      <FileUp className="w-4 h-4 text-muted-foreground shrink-0" aria-hidden="true" />
      <p className="flex-1 text-muted-foreground">
        Gmail sync isn't connected right now. Upload statements directly to keep your transactions up to date.
      </p>
      <Button variant="outline" size="sm" onClick={() => navigate('/statements')} aria-label="Go to statement upload">
        Upload Statements
      </Button>
      <button
        onClick={() => setDismissed(true)}
        aria-label="Dismiss statement-only mode notice"
        className="text-muted-foreground hover:text-foreground"
      >
        <X className="w-4 h-4" aria-hidden="true" />
      </button>
    </div>
  );
}
