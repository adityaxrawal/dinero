/**
 * Compact instrument dropdown for inline use.
 */
import React, { useState, useMemo } from 'react';
import { Pencil, Search, X, CreditCard, Landmark, Zap } from 'lucide-react';
import { Select, SelectTrigger, SelectContent } from '@/components/ui/select';
import { InfoRow } from '@/components/ui/InfoRow';
import { instrumentIcon } from './instrumentTypes';
import { getInstrumentTitle, getInstrumentSubtitle } from './instrumentLabels';
import { InstrumentOptionGroup } from './instrumentSelectParts';

interface InstrumentSelectProps {
  instrumentId: string;
  onInstrumentChange: (id: string) => void;
  instruments: Array<{
    id: string;
    issuer_name: string;
    instrument_type: string;
    masked_identifier?: string | null;
  }>;
}

/** Compact instrument dropdown for inline use. */
export function InstrumentSelect({
  instrumentId,
  onInstrumentChange,
  instruments,
}: InstrumentSelectProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const selectedInstrument = instruments.find((i) => i.id === instrumentId);

  const filtered = useMemo(() => {
    if (!searchQuery.trim()) return instruments;
    const q = searchQuery.toLowerCase().trim();
    return instruments.filter((inst) => {
      const title = getInstrumentTitle(inst).toLowerCase();
      const subtitle = getInstrumentSubtitle(inst).toLowerCase();
      return (
        title.includes(q) || subtitle.includes(q) || inst.instrument_type.toLowerCase().includes(q)
      );
    });
  }, [instruments, searchQuery]);

  const creditCards = useMemo(
    () => filtered.filter((i) => i.instrument_type === 'credit_card'),
    [filtered]
  );
  const bankAccounts = useMemo(
    () =>
      filtered.filter((i) => ['bank_account', 'checking', 'savings'].includes(i.instrument_type)),
    [filtered]
  );
  const upiAndOthers = useMemo(
    () =>
      filtered.filter(
        (i) => !['credit_card', 'bank_account', 'checking', 'savings'].includes(i.instrument_type)
      ),
    [filtered]
  );

  return (
    <InfoRow
      icon={instrumentIcon(selectedInstrument?.instrument_type || '', 14)}
      label="Instrument"
    >
      <Select value={instrumentId} onValueChange={onInstrumentChange}>
        <SelectTrigger
          hideChevron
          className="border-none bg-transparent h-auto p-0 shadow-none hover:bg-transparent focus:ring-0 focus:outline-none data-[state=open]:ring-0 justify-end group font-normal text-right cursor-pointer"
        >
          <div className="flex flex-col items-end text-right">
            <span className="font-semibold text-[13px] text-[#064E3B] underline underline-offset-2 decoration-dashed decoration-[#064E3B]/40 group-hover:decoration-[#064E3B] flex items-center gap-1">
              <span>{getInstrumentTitle(selectedInstrument)}</span>
              <Pencil className="w-3 h-3 text-[#064E3B]/70 shrink-0" />
            </span>
            <span className="text-[11px] text-[#064E3B]/60 font-normal">
              {getInstrumentSubtitle(selectedInstrument)}
            </span>
          </div>
        </SelectTrigger>

        <SelectContent
          hideScrollButtons
          className="bg-[#F8E7C9] border-[#064E3B]/20 text-[#064E3B] shadow-2xl min-w-[340px] max-h-[380px] p-2 rounded-2xl"
        >
          <div className="px-2 pt-1 pb-2 border-b border-[#064E3B]/10 space-y-2 mb-1">
            <div className="flex items-center justify-between">
              <span className="text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/70">
                Select Payment Instrument
              </span>
              <span className="text-[10px] font-mono text-[#064E3B]/50">
                {filtered.length} available
              </span>
            </div>

            <div className="relative">
              <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50" />
              <input
                type="text"
                placeholder="Search bank, card, or UPI..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full h-8 pl-8 pr-7 text-[12px] bg-[#F3EBDD] border border-[#064E3B]/15 rounded-xl outline-none text-[#064E3B] placeholder:text-[#064E3B]/40 focus:border-[#064E3B]/40"
              />
              {searchQuery && (
                <button
                  type="button"
                  onClick={() => setSearchQuery('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
                >
                  <X className="w-3 h-3" />
                </button>
              )}
            </div>
          </div>

          {filtered.length === 0 ? (
            <div className="py-6 text-center text-[12px] italic text-[#064E3B]/50">
              No matching instruments found.
            </div>
          ) : (
            <div className="space-y-2 overflow-y-auto max-h-[290px] pr-0.5">
              <InstrumentOptionGroup
                icon={<CreditCard className="w-3 h-3 text-[#064E3B]/60" />}
                label="Credit Cards"
                items={creditCards}
                selectedId={instrumentId}
                showDivider={false}
              />
              <InstrumentOptionGroup
                icon={<Landmark className="w-3 h-3 text-[#064E3B]/60" />}
                label="Bank Accounts"
                items={bankAccounts}
                selectedId={instrumentId}
                showDivider={creditCards.length > 0}
              />
              <InstrumentOptionGroup
                icon={<Zap className="w-3 h-3 text-[#064E3B]/60" />}
                label="UPI & Digital Payments"
                items={upiAndOthers}
                selectedId={instrumentId}
                showDivider={creditCards.length > 0 || bankAccounts.length > 0}
              />
            </div>
          )}
        </SelectContent>
      </Select>
    </InfoRow>
  );
}
