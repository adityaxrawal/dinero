import { useState } from 'react';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { INSTRUMENT_TYPES } from '@/components/instruments/instrumentTypes';
import InstrumentCard from '@/components/instruments/InstrumentCard';
import AddInstrumentModal from '@/components/instruments/AddInstrumentModal';

/**
 * TASK-FE-011 (Doc 30): grid grouped by type (credit/debit/bank/UPI) —
 * previously grouped by issuer name instead (a reasonable but different
 * partition; switched to match the doc's explicit grouping dimension).
 */
export default function Instruments() {
  const { data: instruments = [], isLoading } = useInstrumentsList();
  const [addModalOpen, setAddModalOpen] = useState(false);

  if (isLoading) return <div className="p-8">Loading instruments...</div>;

  const groups = INSTRUMENT_TYPES.map((t) => ({
    ...t,
    items: instruments.filter((i) => i.instrument_type === t.value),
  })).filter((g) => g.items.length > 0);

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Instruments</h1>
          <p className="text-muted-foreground mt-1">
            Manage your connected bank accounts, credit cards, and UPI VPAs.
          </p>
        </div>
        <Button onClick={() => setAddModalOpen(true)}>
          <Plus className="mr-2 h-4 w-4" />
          Add Instrument
        </Button>
      </div>

      {groups.length === 0 ? (
        <p className="text-muted-foreground text-center py-12">No instruments linked yet.</p>
      ) : (
        <div className="space-y-8">
          {groups.map((group) => (
            <section key={group.value} aria-label={group.label}>
              <h2 className="text-lg font-semibold mb-3">{group.label}s</h2>
              <div className="grid gap-4 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3">
                {group.items.map((inst) => (
                  <InstrumentCard key={inst.id} inst={inst} />
                ))}
              </div>
            </section>
          ))}
        </div>
      )}

      <AddInstrumentModal open={addModalOpen} onOpenChange={setAddModalOpen} />
    </div>
  );
}
