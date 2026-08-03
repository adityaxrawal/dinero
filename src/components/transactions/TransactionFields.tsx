import { X, Hash, MapPin, Link2, ShieldCheck, Clock } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { InfoRow } from '@/components/ui/InfoRow';
import type { CanonicalTransaction } from '@/lib/ipc';

/**
 * Editor fields and audit rows shared by the transaction inspector (side panel)
 * and the transaction detail page. Both render the same record, so keeping one
 * copy here stops the two screens drifting apart.
 */

const FIELD_INPUT_CLASS =
  'h-9 text-[13px] font-semibold bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl pr-8';

export function MerchantField({
  id,
  merchant,
  onChange,
  onSubmit,
}: {
  id: string;
  merchant: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  return (
    <div className="relative">
      <Input
        id={id}
        value={merchant}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Merchant name…"
        className={FIELD_INPUT_CLASS}
        onKeyDown={(e) => e.key === 'Enter' && onSubmit()}
      />
      {merchant && (
        <button
          type="button"
          onClick={() => onChange('')}
          className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/40 hover:text-[#064E3B]"
          aria-label="Clear merchant name"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      )}
    </div>
  );
}

/** "Tags" label with the live count beside it. */
export function TagsHeader({ count }: { count: number }) {
  return (
    <div className="flex items-center justify-between">
      <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">Tags</Label>
      <span className="text-[10px] text-[#064E3B]/50 font-mono">
        {count} tag{count !== 1 ? 's' : ''}
      </span>
    </div>
  );
}

export function EmptyTagsNotice() {
  return <span className="text-[12px] italic text-[#064E3B]/40">No tags added yet.</span>;
}

function StatusBadge({ status }: { status: string | null | undefined }) {
  const isPosted = (status ?? '').toLowerCase() === 'posted';
  return (
    <span
      className="px-2.5 py-0.5 text-[11px] font-bold rounded-full uppercase tracking-wider"
      style={{
        background: isPosted ? 'rgba(16,185,129,0.15)' : 'rgba(107,138,127,0.15)',
        color: isPosted ? '#059669' : '#064E3B',
      }}
    >
      {status ?? 'UNKNOWN'}
    </span>
  );
}

/** The audit / technical-spec rows, rendered inside whichever card wraps them. */
export function TransactionAuditRows({ tx }: { tx: CanonicalTransaction }) {
  return (
    <>
      <InfoRow label="Status">
        <StatusBadge status={tx.status} />
      </InfoRow>

      {tx.best_posting_date && (
        <InfoRow icon={<Clock className="w-3.5 h-3.5" />} label="Posting Date">
          {tx.best_posting_date}
        </InfoRow>
      )}

      {tx.reference_id && (
        <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Reference ID" copyValue={tx.reference_id}>
          <span className="font-mono text-[12px]">{tx.reference_id}</span>
        </InfoRow>
      )}

      <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Transaction ID" copyValue={tx.id}>
        <span className="font-mono text-[11px] opacity-90">{tx.id}</span>
      </InfoRow>

      {tx.location && (
        <InfoRow icon={<MapPin className="w-3.5 h-3.5" />} label="Location">
          {tx.location}
        </InfoRow>
      )}

      {tx.source_mix && (
        <InfoRow icon={<Link2 className="w-3.5 h-3.5" />} label="Source Pipeline">
          <span className="font-mono text-[11px] uppercase bg-[#064E3B]/5 px-2 py-0.5 rounded border border-[#064E3B]/10">
            {tx.source_mix}
          </span>
        </InfoRow>
      )}

      {tx.match_confidence && (
        <InfoRow icon={<ShieldCheck className="w-3.5 h-3.5" />} label="Match Confidence">
          <span className="capitalize">{tx.match_confidence}</span>
        </InfoRow>
      )}

      {tx.event_time_confidence && (
        <InfoRow label="Time Confidence">
          <span className="capitalize">{tx.event_time_confidence}</span>
        </InfoRow>
      )}

      {tx.alert_fired !== null && <InfoRow label="Alert Sent">{tx.alert_fired ? 'Yes' : 'No'}</InfoRow>}
    </>
  );
}
