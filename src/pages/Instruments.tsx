/**
 * Lists all payment instruments with portfolio-level totals.
 */
import { useState, useMemo, useEffect } from 'react';
import { Loader2, Landmark } from 'lucide-react';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { INSTRUMENT_TYPES } from '@/components/instruments/instrumentTypes';
import AddInstrumentModal from '@/components/instruments/AddInstrumentModal';
import InstrumentInspector from '@/components/instruments/InstrumentInspector';
import { Button } from '@/components/ui/button';
import { portfolioMetrics } from './instruments/portfolioMetrics';
import InstrumentListItem from './instruments/InstrumentListItem';
import InstrumentsSidebarHeader, {
  type CategoryFilter,
} from './instruments/InstrumentsSidebarHeader';

/** Lists instruments with portfolio-level totals. */
export default function Instruments() {
  const { data: instruments = [], isLoading } = useInstrumentsList();
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [selectedInstId, setSelectedInstId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedFilter, setSelectedFilter] = useState<CategoryFilter>('all');

  useEffect(() => {
    if (!selectedInstId && instruments.length > 0) {
      setSelectedInstId(instruments[0].id);
    }
  }, [instruments, selectedInstId]);

  const metrics = useMemo(() => portfolioMetrics(instruments), [instruments]);

  const filteredInstruments = useMemo(() => {
    return instruments.filter((i) => {
      if (selectedFilter !== 'all' && i.instrument_type !== selectedFilter) {
        return false;
      }

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

  return (
    <div className="flex h-full w-full overflow-hidden">
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20 shadow-xs"
        style={{ width: '340px', backgroundColor: 'var(--bg-canvas)' }}
      >
        <InstrumentsSidebarHeader
          instruments={instruments}
          metrics={metrics}
          searchQuery={searchQuery}
          setSearchQuery={setSearchQuery}
          selectedFilter={selectedFilter}
          setSelectedFilter={setSelectedFilter}
          onAdd={() => setAddModalOpen(true)}
        />

        <div className="flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading accounts...</span>
            </div>
          ) : groups.length === 0 ? (
            <div className="text-center py-10 px-4 space-y-2">
              <p className="text-xs text-[#064E3B]/50">
                No instruments yet or none match your criteria.
              </p>
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

                  {group.items.map((inst) => (
                    <InstrumentListItem
                      key={inst.id}
                      inst={inst}
                      isSelected={selectedInstId === inst.id}
                      onSelect={() => setSelectedInstId(inst.id)}
                    />
                  ))}
                </div>
              ))}
            </nav>
          )}
        </div>
      </div>

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
