/**
 * Mail scanning controls and progress.
 */
import { useState } from 'react';
import { ScanLine } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import SectionHeading from '@/components/settings/SectionHeading';
import { useGlobalState } from '@/lib/GlobalStateContext';
import ScanRangePicker from './ScanRangePicker';
import ScanProgressPanel from './ScanProgressPanel';
import ScanControls from './ScanControls';

/** Mail scanning controls and progress. */
export default function MailScanSection() {
  const { scanStatus, scanProgress, scanStartedAt, scanFinishedAt, scanError, connectedAccounts } =
    useGlobalState();
  const connectedAccount = connectedAccounts[0] ?? null;
  const { toast } = useToast();
  const [checking, setChecking] = useState(false);
  // Fetches new mail now rather than at the next background cycle (owner
  // decision Q-11: the command existed with no button).
  const checkNow = () => {
    setChecking(true);
    API.ingestion
      .checkNow()
      .then(() => toast({ title: 'Checked for new emails', description: 'Anything new is being processed.' }))
      .catch((err) => toast({ variant: 'destructive', ...getErrorToast(err) }))
      .finally(() => setChecking(false));
  };

  return (
    <section>
      <SectionHeading
        icon={ScanLine}
        title="Mail Scan"
        description="Scan your Gmail inbox for financial emails within a custom date range. The scan runs locally."
      />

      <div className="space-y-6">
        {!connectedAccount && (
          <div className="p-4 rounded-xl bg-amber-500/10 border border-amber-500/20 text-[13px] font-semibold text-amber-700">
            ⚠️ Connect a Gmail account above before scanning.
          </div>
        )}

        <ScanRangePicker disabled={scanStatus === 'running' || !connectedAccount} />

        {connectedAccount && (
          <p className="text-[12px] text-[#064E3B]/60">
            Scanning account:{' '}
            <strong className="font-semibold text-[#064E3B]">{connectedAccount.email}</strong>
          </p>
        )}

        {(scanStatus === 'running' || scanProgress) && (
          <ScanProgressPanel
            scanStatus={scanStatus}
            scanProgress={scanProgress}
            scanStartedAt={scanStartedAt}
            scanFinishedAt={scanFinishedAt}
            scanError={scanError}
          />
        )}

        <ScanControls hasAccount={connectedAccount != null} />

        {connectedAccount && (
          <Button
            variant="outline"
            className="h-9 text-[13px] border-[#064E3B]/20 text-[#064E3B]"
            onClick={checkNow}
            disabled={checking || scanStatus === 'running'}
          >
            {checking ? 'Checking…' : 'Check for new emails now'}
          </Button>
        )}
      </div>
    </section>
  );
}
