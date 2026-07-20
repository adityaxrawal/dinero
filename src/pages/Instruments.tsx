import { useState, useMemo } from 'react';
import { Plus, Loader2, Landmark } from 'lucide-react';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { INSTRUMENT_TYPES, instrumentIcon } from '@/components/instruments/instrumentTypes';
import AddInstrumentModal from '@/components/instruments/AddInstrumentModal';
import InstrumentInspector from '@/components/instruments/InstrumentInspector';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

export default function Instruments() {
  const { data: instruments = [], isLoading } = useInstrumentsList();
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [selectedInstId, setSelectedInstId] = useState<string | null>(null);

  const groups = useMemo(() => {
    return INSTRUMENT_TYPES.map((t) => ({
      ...t,
      items: instruments.filter((i) => i.instrument_type === t.value),
    })).filter((g) => g.items.length > 0);
  }, [instruments]);

  const selectedInst = useMemo(
    () => instruments.find((i) => i.id === selectedInstId),
    [instruments, selectedInstId],
  );

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Master List (Accounts) ─────────────────────────────────── */}
      <div 
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        {/* Header bar */}
        <div
          className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10"
        >
          <div className="flex items-center justify-between">
            <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">
              Accounts
            </h1>

            <div className="flex items-center gap-1">
              <button
                type="button"
                className="flex items-center justify-center w-7 h-7 rounded-md transition-colors bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9]"
                onClick={() => setAddModalOpen(true)}
                aria-label="Add account"
              >
                <Plus className="w-4 h-4" aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading...</span>
            </div>
          ) : groups.length === 0 ? (
            <div className="text-center py-10 px-4">
              <p className="text-xs text-[#064E3B]/50">No accounts linked yet.</p>
              <Button variant="link" className="text-xs h-auto p-0 mt-2 text-[#064E3B]" onClick={() => setAddModalOpen(true)}>Add one</Button>
            </div>
          ) : (
            <nav className="flex flex-col gap-4 py-2">
              {groups.map((group) => (
                <div key={group.value} className="flex flex-col gap-1">
                  <h2 className="px-5 text-[10px] font-semibold uppercase tracking-wider text-[#064E3B]/50 mb-1">
                    {group.label}s
                  </h2>
                  {group.items.map((inst) => {
                    const isSelected = selectedInstId === inst.id;
                    return (
                      <button
                        key={inst.id}
                        onClick={() => setSelectedInstId(inst.id)}
                        className={cn(
                          "flex flex-col w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none",
                          isSelected
                            ? "bg-[#064E3B] text-[#F8E7C9]"
                            : "hover:bg-[#064E3B]/5 text-[#064E3B]"
                        )}
                      >
                        <div className="flex items-center justify-between w-full">
                          <div className="flex items-center gap-2 truncate">
                            <div className={cn("shrink-0", isSelected ? "text-[#F8E7C9]" : "text-[#064E3B]")}>
                              {instrumentIcon(inst.instrument_type, 14)}
                            </div>
                            <span className={cn("text-[13px] font-semibold truncate", isSelected ? "text-white" : "text-[#064E3B]")}>
                              {inst.issuer_name}
                            </span>
                          </div>
                        </div>
                        <span className="text-[11px] mt-0.5 ml-[22px] truncate opacity-70 font-medium">
                          ••{inst.masked_identifier}
                        </span>
                      </button>
                    );
                  })}
                </div>
              ))}
            </nav>
          )}
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ─────────────────────────────────── */}
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
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
            <div className="w-12 h-12 border-2 border-[#064E3B] rounded-xl mb-4 border-dashed flex items-center justify-center">
              <Landmark className="w-6 h-6 text-[#064E3B]" />
            </div>
            <p className="text-[#064E3B] font-medium text-sm">Select an account to view details</p>
          </div>
        )}
      </div>

      <AddInstrumentModal open={addModalOpen} onOpenChange={setAddModalOpen} />
    </div>
  );
}
