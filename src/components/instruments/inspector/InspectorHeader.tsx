import { X, ExternalLink } from 'lucide-react';
import { cn } from '@/lib/utils';

const ICON_BUTTON =
  'w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B] cursor-pointer';

export default function InspectorHeader({
  issuerName,
  maskedIdentifier,
  inline,
  onOpenFullPage,
  onClose,
}: {
  issuerName: string;
  maskedIdentifier: string | null | undefined;
  inline: boolean;
  onOpenFullPage: () => void;
  onClose: () => void;
}) {
  return (
    <div
      className={cn('flex items-center justify-between p-5 flex-shrink-0', inline && 'pt-4')}
      style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
    >
      <div className="flex items-center gap-3 min-w-0">
        <div className="w-9 h-9 rounded-xl bg-[#064E3B] text-[#F8E7C9] flex items-center justify-center text-sm font-bold shrink-0">
          {issuerName.charAt(0).toUpperCase()}
        </div>
        <div className="min-w-0">
          <h2 className="text-[15px] font-bold text-[#064E3B] truncate">{issuerName}</h2>
          <p className="text-[11px] font-mono text-[#064E3B]/60 truncate">••{maskedIdentifier}</p>
        </div>
      </div>

      <div className="flex items-center gap-1.5 shrink-0">
        <button
          type="button"
          className={ICON_BUTTON}
          onClick={onOpenFullPage}
          aria-label="Open full page"
          title="Open full page"
        >
          <ExternalLink className="w-4 h-4" />
        </button>
        <button
          type="button"
          className={ICON_BUTTON}
          onClick={onClose}
          aria-label="Close inspector"
        >
          <X className="w-5 h-5" />
        </button>
      </div>
    </div>
  );
}
