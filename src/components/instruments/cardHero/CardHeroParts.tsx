/**
 * Visual sub-parts of the instrument card hero.
 */
import { AlertCircle, Check, Copy, Wifi } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { InstrumentRecord } from '@/lib/ipc';

/** Issuer and nickname across the top of the card. */
export function CardHeader({
  instrument,
  badgeBg,
}: {
  instrument: InstrumentRecord;
  badgeBg: string;
}) {
  return (
    <div className="flex items-start justify-between mb-5 relative z-10">
      <div className="flex items-center gap-3">
        <div className="w-11 h-11 rounded-xl bg-white/10 backdrop-blur-md flex items-center justify-center text-lg font-black border border-white/20 shadow-inner">
          {instrument.issuer_name.charAt(0).toUpperCase()}
        </div>
        <div>
          <h3 className="font-extrabold text-[17px] tracking-tight text-white flex items-center gap-2">
            {instrument.issuer_name}
          </h3>
          <span className="text-[11px] font-mono text-white/70 uppercase tracking-wider block">
            {instrument.instrument_type.replace('_', ' ')}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <Wifi className="w-4 h-4 text-white/60 rotate-90" />
        <span
          className={cn(
            'px-2.5 py-0.5 rounded-full text-[10px] font-extrabold uppercase tracking-wider border backdrop-blur-md',
            badgeBg
          )}
        >
          {instrument.status}
        </span>
      </div>
    </div>
  );
}

/** Chip and network marks. */
export function ChipRow({ cycleText }: { cycleText: string | null }) {
  return (
    <div className="flex items-center justify-between mb-6 relative z-10">
      <div className="w-11 h-8 rounded-md bg-gradient-to-br from-[#E2C792] via-[#CBB079] to-[#997F48] p-1 shadow-inner border border-[#FFF5DC]/40 flex flex-col justify-between overflow-hidden">
        <div className="w-full h-[1px] bg-black/20" />
        <div className="w-full h-[1px] bg-black/20" />
        <div className="w-full h-[1px] bg-black/20" />
      </div>

      {cycleText && (
        <div className="px-2.5 py-1 rounded-lg bg-white/10 backdrop-blur-md border border-white/15 text-[11px] font-bold text-white/90 font-mono">
          🗓️ {cycleText}
        </div>
      )}
    </div>
  );
}

/** Balance and available credit. */
export function BalanceRow({
  instrument,
  absBalance,
  isNegative,
  copied,
  onCopy,
}: {
  instrument: InstrumentRecord;
  absBalance: number;
  isNegative: boolean;
  copied: boolean;
  onCopy: () => void;
}) {
  const fullId =
    instrument.full_identifier || `•••• •••• •••• ${instrument.masked_identifier || '••••'}`;
  const balanceLabel =
    instrument.instrument_type === 'credit_card'
      ? 'Current Spent / Balance'
      : 'Available Balance';

  return (
    <div className="flex flex-col md:flex-row md:items-end justify-between gap-4 mb-4 relative z-10">
      <div>
        <span className="text-[11px] font-bold uppercase tracking-wider text-white/60 block mb-1">
          {balanceLabel}
        </span>
        <div className="flex items-baseline gap-1.5">
          <span className="text-3xl md:text-4xl font-black font-mono tracking-tight text-white">
            {isNegative ? '−' : ''}₹
            {absBalance.toLocaleString(undefined, { minimumFractionDigits: 2 })}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2 self-start md:self-end bg-black/20 backdrop-blur-md px-3 py-1.5 rounded-xl border border-white/10 group">
        <span className="font-mono text-[13px] tracking-wider text-white/90 font-bold">
          {fullId}
        </span>
        <button
          type="button"
          onClick={onCopy}
          className="p-1 rounded-md text-white/60 hover:text-white hover:bg-white/15 transition-colors cursor-pointer"
          title="Copy identifier"
          aria-label="Copy identifier"
        >
          {copied ? (
            <Check className="w-3.5 h-3.5 text-emerald-400" />
          ) : (
            <Copy className="w-3.5 h-3.5" />
          )}
        </button>
      </div>
    </div>
  );
}

/** Credit utilisation gauge. */
export function UtilizationGauge({
  spent,
  limit,
  ratio,
}: {
  spent: number;
  limit: number;
  ratio: number;
}) {
  const barTone = ratio > 80 ? 'bg-red-400' : ratio > 50 ? 'bg-amber-400' : 'bg-emerald-400';

  return (
    <div className="space-y-1.5 pt-3 border-t border-white/15 relative z-10">
      <div className="flex items-center justify-between text-[11px] font-bold font-mono text-white/80">
        <span>Spent: ₹{spent.toLocaleString()}</span>
        <span className="text-white/60">
          Limit: ₹{limit.toLocaleString()} ({ratio.toFixed(1)}% used)
        </span>
      </div>
      <div className="w-full h-2 rounded-full bg-black/30 overflow-hidden p-0.5 border border-white/10">
        <div
          className={cn('h-full rounded-full transition-all duration-500 shadow-sm', barTone)}
          style={{ width: `${Math.max(4, ratio)}%` }}
        />
      </div>
      {ratio > 80 && (
        <p className="flex items-center gap-1 text-[10px] font-bold text-red-300 pt-0.5">
          <AlertCircle className="w-3 h-3 shrink-0" /> High credit utilization rate on this card.
        </p>
      )}
    </div>
  );
}
