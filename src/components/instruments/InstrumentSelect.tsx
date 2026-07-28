import React, { useState, useMemo } from 'react';
import { Pencil, Search, X, Check, CreditCard, Landmark, Zap } from 'lucide-react';
import { Select, SelectTrigger, SelectContent, SelectItem, SelectGroup, SelectLabel } from '@/components/ui/select';
import { InfoRow } from '@/components/ui/InfoRow';
import { instrumentIcon, instrumentTypeLabel } from './instrumentTypes';
import { cn } from '@/lib/utils';

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

function getInstrumentTitle(inst?: {
  issuer_name?: string | null;
  instrument_type: string;
  masked_identifier?: string | null;
}): string {
  if (!inst) return 'Select Instrument';
  if (inst.issuer_name && inst.issuer_name.trim().length > 0) {
    return inst.issuer_name;
  }
  if (inst.instrument_type === 'upi_vpa' && inst.masked_identifier) {
    const handle = inst.masked_identifier.toLowerCase();
    if (handle.includes('@jupiter')) return 'Jupiter UPI';
    if (handle.includes('@okicici') || handle.includes('@icici')) return 'ICICI UPI';
    if (handle.includes('@okaxis') || handle.includes('@axis')) return 'Axis UPI';
    if (handle.includes('@oksbi') || handle.includes('@sbi')) return 'SBI UPI';
    if (handle.includes('@paytm')) return 'Paytm UPI';
    if (handle.includes('@hdfc')) return 'HDFC UPI';
    return 'UPI Payment Handle';
  }
  return instrumentTypeLabel(inst.instrument_type);
}

function getInstrumentSubtitle(inst?: {
  instrument_type: string;
  masked_identifier?: string | null;
}): string {
  if (!inst) return 'Click to assign';
  const typeLabel = instrumentTypeLabel(inst.instrument_type);
  if (inst.masked_identifier) {
    return `${typeLabel} · ${inst.masked_identifier}`;
  }
  return typeLabel;
}

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
      return title.includes(q) || subtitle.includes(q) || inst.instrument_type.toLowerCase().includes(q);
    });
  }, [instruments, searchQuery]);

  const creditCards = useMemo(() => filtered.filter((i) => i.instrument_type === 'credit_card'), [filtered]);
  const bankAccounts = useMemo(
    () => filtered.filter((i) => ['bank_account', 'checking', 'savings'].includes(i.instrument_type)),
    [filtered]
  );
  const upiAndOthers = useMemo(
    () => filtered.filter((i) => !['credit_card', 'bank_account', 'checking', 'savings'].includes(i.instrument_type)),
    [filtered]
  );

  const renderInstrumentItem = (inst: (typeof instruments)[0]) => {
    const isSelected = inst.id === instrumentId;
    const title = getInstrumentTitle(inst);
    const subtitle = getInstrumentSubtitle(inst);

    return (
      <SelectItem
        key={inst.id}
        value={inst.id}
        hideCheckmark
        className={cn(
          'py-2 px-2.5 my-0.5 rounded-xl transition-all cursor-pointer select-none outline-none pr-3',
          'focus:bg-[#064E3B]/10 focus:text-[#064E3B]',
          isSelected
            ? 'bg-[#064E3B]/[0.10] border border-[#064E3B]/25 font-medium'
            : 'hover:bg-[#064E3B]/[0.05]'
        )}
      >
        <div className="flex items-center justify-between w-full gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <div className="w-8 h-8 rounded-lg bg-[#064E3B]/10 flex items-center justify-center text-[#064E3B] shrink-0 shadow-2xs group-hover:scale-105 transition-transform">
              {instrumentIcon(inst.instrument_type, 16)}
            </div>
            <div className="flex flex-col text-left min-w-0">
              <span className="font-bold text-[13px] text-[#064E3B] leading-tight truncate">
                {title}
              </span>
              <span className="text-[11px] text-[#064E3B]/65 font-medium truncate font-mono">
                {subtitle}
              </span>
            </div>
          </div>

          {isSelected && (
            <div className="w-5 h-5 rounded-full bg-[#064E3B] text-white flex items-center justify-center shrink-0 shadow-2xs">
              <Check className="w-3 h-3" strokeWidth={3} />
            </div>
          )}
        </div>
      </SelectItem>
    );
  };

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

        <SelectContent hideScrollButtons className="bg-[#F8E7C9] border-[#064E3B]/20 text-[#064E3B] shadow-2xl min-w-[340px] max-h-[380px] p-2 rounded-2xl">
          {/* Header & Search Input */}
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
              {/* 💳 Credit Cards */}
              {creditCards.length > 0 && (
                <SelectGroup>
                  <SelectLabel className="flex items-center gap-1.5 text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/70 px-2 py-1">
                    <CreditCard className="w-3 h-3 text-[#064E3B]/60" />
                    <span>Credit Cards</span>
                    <span className="ml-auto text-[9px] px-1.5 py-0.2 rounded-full bg-[#064E3B]/10">
                      {creditCards.length}
                    </span>
                  </SelectLabel>
                  {creditCards.map(renderInstrumentItem)}
                </SelectGroup>
              )}

              {/* 🏦 Bank Accounts */}
              {bankAccounts.length > 0 && (
                <SelectGroup>
                  {creditCards.length > 0 && <div className="border-t border-[#064E3B]/10 my-1" />}
                  <SelectLabel className="flex items-center gap-1.5 text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/70 px-2 py-1">
                    <Landmark className="w-3 h-3 text-[#064E3B]/60" />
                    <span>Bank Accounts</span>
                    <span className="ml-auto text-[9px] px-1.5 py-0.2 rounded-full bg-[#064E3B]/10">
                      {bankAccounts.length}
                    </span>
                  </SelectLabel>
                  {bankAccounts.map(renderInstrumentItem)}
                </SelectGroup>
              )}

              {/* ⚡ UPI & Other Payments */}
              {upiAndOthers.length > 0 && (
                <SelectGroup>
                  {(creditCards.length > 0 || bankAccounts.length > 0) && (
                    <div className="border-t border-[#064E3B]/10 my-1" />
                  )}
                  <SelectLabel className="flex items-center gap-1.5 text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/70 px-2 py-1">
                    <Zap className="w-3 h-3 text-[#064E3B]/60" />
                    <span>UPI & Digital Payments</span>
                    <span className="ml-auto text-[9px] px-1.5 py-0.2 rounded-full bg-[#064E3B]/10">
                      {upiAndOthers.length}
                    </span>
                  </SelectLabel>
                  {upiAndOthers.map(renderInstrumentItem)}
                </SelectGroup>
              )}
            </div>
          )}
        </SelectContent>
      </Select>
    </InfoRow>
  );
}
