/**
 * Blocks write access once a licence is locked.
 *
 * Reads deliberately remain available -- a lapsed subscription must not hold the
 * user's own financial history hostage.
 */
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { useLicenseRefresh } from '@/hooks/useLicenseRefresh';

/**
 * Blocks write access once a licence is locked.
 *
 * Reads remain available -- a lapsed subscription must not hold the user's own
 * financial history hostage.
 */
export default function LicenseLockOverlay() {
  const isLocked = useLicenseStore((s) => s.isLocked);
  const navigate = useNavigate();
  const { isRetrying, handleRetry } = useLicenseRefresh();

  if (!isLocked) return null;

  return (
    <div
      role="alert"
      aria-labelledby="license-locked-title"
      aria-describedby="license-locked-desc"
      className="flex flex-col gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border border-red-400/30 bg-red-400/10"
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className="w-3.5 h-3.5 text-red-300 shrink-0 mt-0.5" aria-hidden="true" />
        <div className="flex-1 text-[11.5px] leading-snug">
          <span id="license-locked-title" className="font-semibold text-red-200">
            License Locked.
          </span>{' '}
          <span id="license-locked-desc" className="text-red-200/80">
            Editing and syncing are paused until you reactivate — you can still browse existing
            data.
          </span>
        </div>
      </div>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={handleRetry}
          disabled={isRetrying}
          aria-label="Retry license validation now"
          className="h-7 text-[11.5px] flex-1 border-red-400/30 text-red-200 bg-transparent hover:bg-red-400/10 hover:text-red-100"
        >
          {isRetrying ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" aria-hidden="true" />
          ) : (
            'Retry'
          )}
        </Button>
        <Button
          variant="default"
          size="sm"
          onClick={() => navigate('/settings')}
          aria-label="Go to Settings to reactivate"
          className="h-7 text-[11.5px] flex-1 bg-[#F8E7C9] text-[#064E3B] hover:bg-[#F8E7C9]/90"
        >
          Reactivate
        </Button>
      </div>
    </div>
  );
}
