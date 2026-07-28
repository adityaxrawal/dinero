import { useState } from 'react';
import { Copy, Check, Wifi, AlertCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import type { InstrumentRecord } from '@/lib/ipc';

interface InstrumentCardHeroProps {
  instrument: InstrumentRecord;
}

export function getBankTheme(issuerName: string) {
  const name = issuerName.toLowerCase();
  if (name.includes('idfc')) {
    return {
      gradient: 'from-[#600C12] via-[#7B1113] to-[#3D0609]',
      accentColor: '#FFD700',
      badgeBg: 'bg-[#FFD700]/15 text-[#FFD700] border-[#FFD700]/30',
      label: 'IDFC FIRST',
    };
  }
  if (name.includes('hdfc')) {
    return {
      gradient: 'from-[#0F3868] via-[#004B8D] to-[#082142]',
      accentColor: '#60A5FA',
      badgeBg: 'bg-blue-400/15 text-blue-300 border-blue-400/30',
      label: 'HDFC Bank',
    };
  }
  if (name.includes('sbi')) {
    return {
      gradient: 'from-[#1E3A8A] via-[#1D4ED8] to-[#172554]',
      accentColor: '#93C5FD',
      badgeBg: 'bg-sky-400/15 text-sky-200 border-sky-400/30',
      label: 'SBI',
    };
  }
  if (name.includes('axis')) {
    return {
      gradient: 'from-[#6B0F38] via-[#97144D] to-[#450923]',
      accentColor: '#F472B6',
      badgeBg: 'bg-pink-400/15 text-pink-300 border-pink-400/30',
      label: 'Axis Bank',
    };
  }
  if (name.includes('jupiter')) {
    return {
      gradient: 'from-[#045C4B] via-[#00897B] to-[#023329]',
      accentColor: '#34D399',
      badgeBg: 'bg-emerald-400/15 text-emerald-300 border-emerald-400/30',
      label: 'Jupiter',
    };
  }
  if (name.includes('yes')) {
    return {
      gradient: 'from-[#1E3A5F] via-[#0055A5] to-[#0D1F38]',
      accentColor: '#60A5FA',
      badgeBg: 'bg-blue-400/15 text-blue-300 border-blue-400/30',
      label: 'Yes Bank',
    };
  }

  // Default Dinero Dark Emerald
  return {
    gradient: 'from-[#064E3B] via-[#043327] to-[#022018]',
    accentColor: '#34D399',
    badgeBg: 'bg-[#F8E7C9]/15 text-[#F8E7C9] border-[#F8E7C9]/20',
    label: issuerName,
  };
}

export default function InstrumentCardHero({ instrument }: InstrumentCardHeroProps) {
  const { toast } = useToast();
  const [copied, setCopied] = useState(false);

  const theme = getBankTheme(instrument.issuer_name);
  const isNegative = (instrument.current_balance ?? 0) < 0;
  const absBalance = Math.abs(instrument.current_balance ?? 0);

  const fullId = instrument.full_identifier || `•••• •••• •••• ${instrument.masked_identifier || '••••'}`;

  const handleCopyIdentifier = () => {
    const textToCopy = instrument.full_identifier || instrument.masked_identifier || '';
    if (!textToCopy) return;
    navigator.clipboard.writeText(textToCopy);
    setCopied(true);
    toast({
      title: 'Copied to clipboard',
      description: `Identifier ${textToCopy} copied.`,
    });
    setTimeout(() => setCopied(false), 2000);
  };

  // Calculate billing cycle countdown if available
  let cycleText: string | null = null;
  if (instrument.instrument_type === 'credit_card' && instrument.billing_cycle_day) {
    const today = new Date();
    const currentDay = today.getDate();
    const cycleDay = instrument.billing_cycle_day;
    let daysLeft = cycleDay - currentDay;
    if (daysLeft < 0) {
      // next month
      const daysInMonth = new Date(today.getFullYear(), today.getMonth() + 1, 0).getDate();
      daysLeft += daysInMonth;
    }
    cycleText = daysLeft === 0 ? 'Bill generated today' : `Bill in ${daysLeft} ${daysLeft === 1 ? 'day' : 'days'}`;
  }

  const creditLimit = instrument.credit_limit ?? 0;
  const hasLimit = instrument.instrument_type === 'credit_card' && creditLimit > 0;
  const utilRatio = hasLimit ? Math.min(100, (absBalance / creditLimit) * 100) : 0;

  return (
    <div
      className={cn(
        'relative overflow-hidden rounded-2xl p-5 md:p-6 text-white shadow-xl border border-white/10 transition-all duration-300 bg-gradient-to-br',
        theme.gradient
      )}
    >
      {/* Background Micro Decorative Grid & Glow */}
      <div className="absolute -right-10 -bottom-10 w-44 h-44 rounded-full bg-white/[0.04] blur-2xl pointer-events-none" />
      <div className="absolute -left-10 -top-10 w-36 h-36 rounded-full bg-white/[0.03] blur-xl pointer-events-none" />

      {/* Card Header: Logo/Issuer Name + Chip + Status */}
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
              theme.badgeBg
            )}
          >
            {instrument.status}
          </span>
        </div>
      </div>

      {/* Realistic Metallic Chip Graphic */}
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

      {/* Card Body: Balance + Number */}
      <div className="flex flex-col md:flex-row md:items-end justify-between gap-4 mb-4 relative z-10">
        <div>
          <span className="text-[11px] font-bold uppercase tracking-wider text-white/60 block mb-1">
            {instrument.instrument_type === 'credit_card' ? 'Current Spent / Balance' : 'Available Balance'}
          </span>
          <div className="flex items-baseline gap-1.5">
            <span className="text-3xl md:text-4xl font-black font-mono tracking-tight text-white">
              {isNegative ? '−' : ''}₹{absBalance.toLocaleString(undefined, { minimumFractionDigits: 2 })}
            </span>
          </div>
        </div>

        {/* Card Number / Identifier with Copy Button */}
        <div className="flex items-center gap-2 self-start md:self-end bg-black/20 backdrop-blur-md px-3 py-1.5 rounded-xl border border-white/10 group">
          <span className="font-mono text-[13px] tracking-wider text-white/90 font-bold">
            {fullId}
          </span>
          <button
            type="button"
            onClick={handleCopyIdentifier}
            className="p-1 rounded-md text-white/60 hover:text-white hover:bg-white/15 transition-colors cursor-pointer"
            title="Copy identifier"
            aria-label="Copy identifier"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          </button>
        </div>
      </div>

      {/* Credit Limit Progress Gauge (if applicable) */}
      {hasLimit && (
        <div className="space-y-1.5 pt-3 border-t border-white/15 relative z-10">
          <div className="flex items-center justify-between text-[11px] font-bold font-mono text-white/80">
            <span>Spent: ₹{absBalance.toLocaleString()}</span>
            <span className="text-white/60">
              Limit: ₹{creditLimit.toLocaleString()} ({utilRatio.toFixed(1)}% used)
            </span>
          </div>
          <div className="w-full h-2 rounded-full bg-black/30 overflow-hidden p-0.5 border border-white/10">
            <div
              className={cn(
                'h-full rounded-full transition-all duration-500 shadow-sm',
                utilRatio > 80 ? 'bg-red-400' : utilRatio > 50 ? 'bg-amber-400' : 'bg-emerald-400'
              )}
              style={{ width: `${Math.max(4, utilRatio)}%` }}
            />
          </div>
          {utilRatio > 80 && (
            <p className="flex items-center gap-1 text-[10px] font-bold text-red-300 pt-0.5">
              <AlertCircle className="w-3 h-3 shrink-0" /> High credit utilization rate on this card.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
