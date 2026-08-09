import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import type { InstrumentRecord } from '@/lib/ipc';
import { inspectorPanelClasses, inspectorPanelStyle } from '@/components/layout/inspectorPanel';
import { useInstrumentForm } from './useInstrumentForm';
import InspectorHeader from './inspector/InspectorHeader';
import InspectorTabs, { type Tab } from './inspector/InspectorTabs';
import InspectorBody from './inspector/InspectorBody';

interface InstrumentInspectorProps {
  instrument: InstrumentRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

export default function InstrumentInspector({
  instrument,
  onClose,
  inline = false,
}: InstrumentInspectorProps) {
  const navigate = useNavigate();
  const { toast } = useToast();
  const isOpen = !!instrument;

  const [activeTab, setActiveTab] = useState<Tab>('details');
  const [txSearchQuery, setTxSearchQuery] = useState('');
  const [copiedId, setCopiedId] = useState(false);

  const form = useInstrumentForm(instrument?.id, instrument, onClose);
  const { inst } = form;

  // Reset tab when instrument changes
  useEffect(() => {
    setActiveTab('details');
    setTxSearchQuery('');
  }, [instrument?.id]);

  const handleCopyAccountId = () => {
    if (!inst?.id) return;
    navigator.clipboard.writeText(inst.id);
    setCopiedId(true);
    toast({ title: 'Account ID Copied', description: `Copied ${inst.id} to clipboard.` });
    setTimeout(() => setCopiedId(false), 2000);
  };

  if (!inst) return null;

  return (
    <aside
      className={inspectorPanelClasses(inline, isOpen)}
      role="complementary"
      aria-label="Account detail"
      aria-hidden={!isOpen}
      style={inspectorPanelStyle(inline, isOpen)}
    >
      <InspectorHeader
        issuerName={inst.issuer_name}
        maskedIdentifier={inst.masked_identifier}
        inline={inline}
        onOpenFullPage={() => navigate(`/instruments/${inst.id}`)}
        onClose={onClose}
      />

      <InspectorTabs
        activeTab={activeTab}
        onSelect={setActiveTab}
        counts={{
          transactions: form.totalTxCount || form.recentTransactions.length,
          statements: form.instrumentStatements.length,
        }}
      />

      <div className={cn('flex-1 overflow-y-auto', inline ? 'p-6 max-w-4xl mx-auto w-full' : 'p-4')}>
        <InspectorBody
          form={form}
          inst={inst}
          activeTab={activeTab}
          copiedId={copiedId}
          onCopyAccountId={handleCopyAccountId}
          txSearchQuery={txSearchQuery}
          onTxSearchChange={setTxSearchQuery}
        />
      </div>
    </aside>
  );
}
