import { useState } from 'react';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import type { InstrumentRecord } from '@/lib/ipc';
import { getBankTheme } from './cardHero/bankTheme';
import { billingCycleText } from './cardHero/billingCycle';
import {
  CardHeader,
  ChipRow,
  BalanceRow,
  UtilizationGauge,
} from './cardHero/CardHeroParts';

interface InstrumentCardHeroProps {
  instrument: InstrumentRecord;
}

export default function InstrumentCardHero({ instrument }: InstrumentCardHeroProps) {
  const { toast } = useToast();
  const [copied, setCopied] = useState(false);

  const theme = getBankTheme(instrument.issuer_name);
  const balance = instrument.current_balance ?? 0;
  const absBalance = Math.abs(balance);

  const handleCopyIdentifier = () => {
    const textToCopy = instrument.full_identifier || instrument.masked_identifier || '';
    if (!textToCopy) return;
    navigator.clipboard.writeText(textToCopy);
    setCopied(true);
    toast({ title: 'Copied to clipboard', description: `Identifier ${textToCopy} copied.` });
    setTimeout(() => setCopied(false), 2000);
  };

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

      <CardHeader instrument={instrument} badgeBg={theme.badgeBg} />

      <ChipRow
        cycleText={billingCycleText(instrument.instrument_type, instrument.billing_cycle_day)}
      />

      <BalanceRow
        instrument={instrument}
        absBalance={absBalance}
        isNegative={balance < 0}
        copied={copied}
        onCopy={handleCopyIdentifier}
      />

      {hasLimit && (
        <UtilizationGauge spent={absBalance} limit={creditLimit} ratio={utilRatio} />
      )}
    </div>
  );
}
