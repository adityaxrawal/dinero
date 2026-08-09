import { Loader2 } from 'lucide-react';
import type { InstrumentRecord } from '@/lib/ipc';
import type { useInstrumentForm } from '../useInstrumentForm';
import InstrumentCardHero from '../InstrumentCardHero';
import InstrumentAnalyticsTab from '../InstrumentAnalyticsTab';
import type { Tab } from './InspectorTabs';
import DetailsTab from './DetailsTab';
import TransactionsTab from './TransactionsTab';
import StatementsTab from './StatementsTab';

type Form = ReturnType<typeof useInstrumentForm>;

interface InspectorBodyProps {
  form: Form;
  inst: InstrumentRecord;
  activeTab: Tab;
  copiedId: boolean;
  onCopyAccountId: () => void;
  txSearchQuery: string;
  onTxSearchChange: (q: string) => void;
}

export default function InspectorBody({
  form,
  inst,
  activeTab,
  copiedId,
  onCopyAccountId,
  txSearchQuery,
  onTxSearchChange,
}: InspectorBodyProps) {
  if (form.isLoading && !form.detailInst) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2 className="w-6 h-6 animate-spin text-[#064E3B]" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <InstrumentCardHero instrument={inst} />

      {activeTab === 'details' && (
        <DetailsTab
          form={form}
          inst={inst}
          copiedId={copiedId}
          onCopyAccountId={onCopyAccountId}
        />
      )}

      {activeTab === 'transactions' && (
        <TransactionsTab
          form={form}
          instrumentId={inst.id}
          searchQuery={txSearchQuery}
          onSearchChange={onTxSearchChange}
        />
      )}

      {activeTab === 'statements' && <StatementsTab statements={form.instrumentStatements} />}

      {activeTab === 'analytics' && (
        <InstrumentAnalyticsTab transactions={form.recentTransactions} />
      )}
    </div>
  );
}
