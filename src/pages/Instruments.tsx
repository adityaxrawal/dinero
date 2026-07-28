import { useState, useMemo, useEffect } from 'react';
import { Plus, Loader2, Landmark, Search, X, CreditCard, Wallet, Smartphone, ShieldAlert } from 'lucide-react';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { INSTRUMENT_TYPES, instrumentIcon } from '@/components/instruments/instrumentTypes';
import AddInstrumentModal from '@/components/instruments/AddInstrumentModal';
import InstrumentInspector from '@/components/instruments/InstrumentInspector';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type CategoryFilter = 'all' | 'credit_card' | 'bank_account' | 'upi_vpa';

export default function Instruments() {
  const { data: instruments = [], isLoading } = useInstrumentsList();
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [selectedInstId, setSelectedInstId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedFilter, setSelectedFilter] = useState<CategoryFilter>('all');

  // Auto select first instrument if none selected and instruments exist
  useEffect(() => {
    if (!selectedInstId && instruments.length > 0) {
      setSelectedInstId(instruments[0].id);
    }
  }, [instruments, selectedInstId]);

  // Compute overall portfolio metrics
  const summaryMetrics = useMemo(() => {
    let totalBankBalance = 0;
    let totalCreditSpent = 0;
    let totalCreditLimit = 0;

    instruments.forEach((inst) => {
      const bal = inst.current_balance ?? 0;
      if (inst.instrument_type === 'credit_card') {
        totalCreditSpent += Math.abs(bal);
        if (inst.credit_limit) {
          totalCreditLimit += inst.credit_limit;
        }
      } else if (inst.instrument_type === 'bank_account' || inst.instrument_type === 'wallet') {
        totalBankBalance += bal;
      }
    });

    return {
      totalBankBalance,
      totalCreditSpent,
      totalCreditLimit,
      count: instruments.length,
    };
  }, [instruments]);

  const filteredInstruments = useMemo(() => {
    return instruments.filter((i) => {
      // Filter by type pill
      if (selectedFilter !== 'all' && i.instrument_type !== selectedFilter) {
        return false;
      }

      // Filter by search query
      if (searchQuery.trim()) {
        const q = searchQuery.toLowerCase().trim();
        const matchesName = i.issuer_name.toLowerCase().includes(q);
        const matchesMask = i.masked_identifier && i.masked_identifier.toLowerCase().includes(q);
        const matchesType = i.instrument_type.toLowerCase().includes(q);
        return matchesName || matchesMask || matchesType;
      }

      return true;
    });
  }, [instruments, searchQuery, selectedFilter]);

  const groups = useMemo(() => {
    return INSTRUMENT_TYPES.map((t) => ({
      ...t,
      items: filteredInstruments.filter((i) => i.instrument_type === t.value),
    })).filter((g) => g.items.length > 0);
  }, [filteredInstruments]);

  const selectedInst = useMemo(
    () => instruments.find((i) => i.id === selectedInstId),
    [instruments, selectedInstId]
  );

  const categoryPills: { id: CategoryFilter; label: string; count: number; icon: React.ReactNode }[] = [
    {
      id: 'all',
      label: 'All',
      count: instruments.length,
      icon: <Wallet className="w-3 h-3" />,
    },
    {
      id: 'credit_card',
      label: 'Cards',
      count: instruments.filter((i) => i.instrument_type === 'credit_card').length,
      icon: <CreditCard className="w-3 h-3" />,
    },
    {
      id: 'bank_account',
      label: 'Banks',
      count: instruments.filter((i) => i.instrument_type === 'bank_account').length,
      icon: <Landmark className="w-3 h-3" />,
    },
    {
      id: 'upi_vpa',
      label: 'UPI',
      count: instruments.filter((i) => i.instrument_type === 'upi_vpa').length,
      icon: <Smartphone className="w-3 h-3" />,
    },
  ];

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Master List (Accounts) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20 shadow-xs"
        style={{ width: '340px', backgroundColor: 'var(--bg-canvas)' }}
      >
        {/* Header bar & Portfolio Metrics */}
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h1 className="text-[15px] font-extrabold text-[#064E3B] tracking-tight">Accounts</h1>
              <span className="text-[10px] font-extrabold px-2 py-0.5 rounded-full bg-[#064E3B]/10 text-[#064E3B]">
                {instruments.length}
              </span>
            </div>

            <button
              type="button"
              className="flex items-center gap-1 px-2.5 py-1 rounded-xl text-xs font-bold transition-all bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] cursor-pointer shadow-xs"
              onClick={() => setAddModalOpen(true)}
              aria-label="Add account"
            >
              <Plus className="w-3.5 h-3.5" aria-hidden="true" />
              <span>Add</span>
            </button>
          </div>

          {/* Quick Portfolio Stats Banner */}
          <div className="bg-[#064E3B]/[0.06] rounded-xl p-2.5 border border-[#064E3B]/10 flex justify-between items-center text-[11px] font-mono">
            <div>
              <span className="text-[#064E3B]/60 font-medium block text-[9px] uppercase tracking-wider">
                Bank Balance
              </span>
              <span className="font-bold text-[#064E3B] text-xs">
                ₹{summaryMetrics.totalBankBalance.toLocaleString()}
              </span>
            </div>
            <div className="h-6 w-[1px] bg-[#064E3B]/15" />
            <div className="text-right">
              <span className="text-[#064E3B]/60 font-medium block text-[9px] uppercase tracking-wider">
                Credit Spent
              </span>
              <span className="font-bold text-red-700 text-xs">
                ₹{summaryMetrics.totalCreditSpent.toLocaleString()}
              </span>
            </div>
          </div>

          {/* Search Bar */}
          <div className="relative">
            <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-[#064E3B]/50" />
            <input
              type="text"
              placeholder="Search accounts, cards..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full h-8 pl-8 pr-7 text-[12px] bg-[#F3EBDD]/80 border border-[#064E3B]/15 rounded-xl outline-none text-[#064E3B] placeholder:text-[#064E3B]/40 focus:border-[#064E3B]/40"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
              >
                <X className="w-3 h-3" />
              </button>
            )}
          </div>

          {/* Category Filter Pills */}
          <div className="grid grid-cols-4 gap-1">
            {categoryPills.map((pill) => (
              <button
                key={pill.id}
                type="button"
                onClick={() => setSelectedFilter(pill.id)}
                className={cn(
                  'flex items-center justify-center gap-1 px-1.5 py-1 text-[11px] font-bold rounded-lg transition-all cursor-pointer border truncate',
                  selectedFilter === pill.id
                    ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-2xs'
                    : 'bg-[#F3EBDD]/50 text-[#064E3B]/70 border-[#064E3B]/10 hover:bg-[#064E3B]/10 hover:text-[#064E3B]'
                )}
                title={`${pill.label} (${pill.count})`}
              >
                <span className="shrink-0">{pill.icon}</span>
                <span className="truncate">{pill.label}</span>
                <span className="opacity-75 text-[9px] font-mono shrink-0 font-normal">({pill.count})</span>
              </button>
            ))}
          </div>
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading accounts...</span>
            </div>
          ) : groups.length === 0 ? (
            <div className="text-center py-10 px-4 space-y-2">
              <p className="text-xs text-[#064E3B]/50">No accounts match your criteria.</p>
              <Button
                variant="link"
                className="text-xs h-auto p-0 text-[#064E3B] font-bold"
                onClick={() => {
                  setSearchQuery('');
                  setSelectedFilter('all');
                  setAddModalOpen(true);
                }}
              >
                + Add a new account
              </Button>
            </div>
          ) : (
            <nav className="flex flex-col gap-4 py-3">
              {groups.map((group) => (
                <div key={group.value} className="flex flex-col gap-1">
                  <div className="flex items-center justify-between px-4 mb-1">
                    <h2 className="text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/60">
                      {group.label}s
                    </h2>
                    <span className="text-[9px] font-bold px-1.5 py-0.2 rounded-full bg-[#064E3B]/10 text-[#064E3B]">
                      {group.items.length}
                    </span>
                  </div>

                  {group.items.map((inst) => {
                    const isSelected = selectedInstId === inst.id;
                    const bal = inst.current_balance ?? 0;
                    const isNeg = bal < 0;

                    return (
                      <button
                        key={inst.id}
                        onClick={() => setSelectedInstId(inst.id)}
                        className={cn(
                          'flex items-center justify-between w-full text-left px-3 py-2.5 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border',
                          isSelected
                            ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-md ring-1 ring-[#064E3B]/20'
                            : 'hover:bg-[#064E3B]/[0.06] text-[#064E3B] border-transparent'
                        )}
                      >
                        <div className="flex items-center gap-2.5 min-w-0 pr-1">
                          <div
                            className={cn(
                              'w-8 h-8 rounded-lg flex items-center justify-center shrink-0 text-xs font-bold transition-colors',
                              isSelected
                                ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
                                : 'bg-[#064E3B]/10 text-[#064E3B]'
                            )}
                          >
                            {instrumentIcon(inst.instrument_type, 14)}
                          </div>
                          <div className="flex flex-col min-w-0">
                            <span
                              className={cn(
                                'text-[13px] font-bold tracking-tight truncate',
                                isSelected ? 'text-white' : 'text-[#064E3B]'
                              )}
                            >
                              {inst.issuer_name || 'Account'}
                            </span>
                            <span
                              className={cn(
                                'text-[11px] font-mono font-medium truncate opacity-70',
                                isSelected ? 'text-[#F8E7C9]' : 'text-[#064E3B]/70'
                              )}
                            >
                              ••{inst.masked_identifier}
                            </span>
                          </div>
                        </div>

                        {/* Balance Preview Pill */}
                        <div className="text-right shrink-0">
                          <span
                            className={cn(
                              'text-[12px] font-extrabold font-mono tracking-tight block',
                              isSelected
                                ? 'text-white'
                                : isNeg
                                ? 'text-red-700'
                                : 'text-[#064E3B]'
                            )}
                          >
                            ₹{Math.abs(bal).toLocaleString(undefined, {
                              minimumFractionDigits: 2,
                              maximumFractionDigits: 2,
                            })}
                          </span>
                        </div>
                      </button>
                    );
                  })}
                </div>
              ))}
            </nav>
          )}
        </div>
      </div>

      {/* ── Inspector Panel Canvas ─────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col justify-center">
        {selectedInstId ? (
          <div className="w-full h-full flex flex-col">
            <InstrumentInspector
              instrument={selectedInst}
              onClose={() => setSelectedInstId(null)}
              inline={true}
            />
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-40 space-y-3">
            <div className="w-14 h-14 border-2 border-[#064E3B] rounded-2xl border-dashed flex items-center justify-center bg-[#064E3B]/5">
              <Landmark className="w-7 h-7 text-[#064E3B]" />
            </div>
            <p className="text-[#064E3B] font-bold text-sm">Select an account to view details</p>
          </div>
        )}
      </div>

      <AddInstrumentModal open={addModalOpen} onOpenChange={setAddModalOpen} />
    </div>
  );
}
