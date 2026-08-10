/**
 * Indicates the app is running without Gmail, on manually uploaded statements alone.
 */
import { useState, useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { FileUp, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** Indicates the app is running on manually uploaded statements alone. */
export default function StatementOnlyModeBanner() {
  const { connectedAccounts } = useGlobalState();
  const location = useLocation();
  const navigate = useNavigate();
  const [dismissed, setDismissed] = useState(false);

  const hasNoWorkingGmailAccount = connectedAccounts.every(
    (a) => a.account_status?.toLowerCase() !== 'active'
  );

  useEffect(() => {
    if (!hasNoWorkingGmailAccount) setDismissed(false);
  }, [hasNoWorkingGmailAccount]);

  if (!hasNoWorkingGmailAccount || dismissed || location.pathname === '/statements') return null;

  return (
    <div
      role="status"
      className="flex flex-col gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border border-[#F8E7C9]/15 bg-[#F8E7C9]/5"
    >
      <div className="flex items-start gap-2">
        <FileUp className="w-3.5 h-3.5 text-[#F8E7C9]/70 shrink-0 mt-0.5" aria-hidden="true" />
        <p className="flex-1 text-[11.5px] leading-snug text-[#F8E7C9]/70">
          Gmail sync isn't connected. Upload statements directly to keep transactions up to date.
        </p>
        <button
          onClick={() => setDismissed(true)}
          aria-label="Dismiss statement-only mode notice"
          className="text-[#F8E7C9]/40 hover:text-[#F8E7C9]/80 shrink-0"
        >
          <X className="w-3.5 h-3.5" aria-hidden="true" />
        </button>
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={() => navigate('/statements')}
        aria-label="Go to statement upload"
        className="h-7 text-[11.5px] w-full border-[#F8E7C9]/25 text-[#F8E7C9] bg-transparent hover:bg-[#F8E7C9]/10 hover:text-[#F8E7C9]"
      >
        Upload Statements
      </Button>
    </div>
  );
}
