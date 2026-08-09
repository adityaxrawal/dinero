import { CheckCircle2 } from 'lucide-react';
import type { InstrumentRecord } from '@/lib/ipc';
import type { useInstrumentForm } from '../useInstrumentForm';
import IdentityCard from './IdentityCard';
import SecurityCard from './SecurityCard';
import BillingCard from './BillingCard';
import MetadataCard from './MetadataCard';
import PasswordVaultList from './PasswordVaultList';
import SaveDeleteBar from './SaveDeleteBar';

type Form = ReturnType<typeof useInstrumentForm>;

export default function DetailsTab({
  form,
  inst,
  copiedId,
  onCopyAccountId,
}: {
  form: Form;
  inst: InstrumentRecord;
  copiedId: boolean;
  onCopyAccountId: () => void;
}) {
  const editable = { fields: form.fields, setField: form.setField, onSave: form.handleSave };

  return (
    <div className="space-y-5 animate-in fade-in-50 duration-200">
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <IdentityCard
          {...editable}
          accountId={inst.id}
          copied={copiedId}
          onCopyAccountId={onCopyAccountId}
        />
        <SecurityCard {...editable} />
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <BillingCard {...editable} currentBalance={inst.current_balance} />
        <MetadataCard {...editable} />
      </div>

      {form.instrumentPasswords.length > 0 && (
        <PasswordVaultList
          passwords={form.instrumentPasswords}
          onForget={(id) => form.forgetPassword.mutate(id)}
          isForgetting={form.forgetPassword.isPending}
        />
      )}

      {form.showSavedConfirm && (
        <p className="flex items-center gap-1.5 text-xs font-bold text-emerald-600">
          <CheckCircle2 className="w-3.5 h-3.5" /> Changes saved successfully.
        </p>
      )}

      <SaveDeleteBar
        isSaving={form.isSaving}
        isDeleting={form.isDeleting}
        onSave={form.handleSave}
        onDelete={form.handleDelete}
      />
    </div>
  );
}
