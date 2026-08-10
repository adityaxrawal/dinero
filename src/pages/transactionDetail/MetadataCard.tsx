/**
 * Transaction metadata: reference, channel, timestamps.
 */
import { CheckCircle2 } from 'lucide-react';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { MerchantField } from '@/components/transactions/TransactionFields';
import { CategorySelect } from '@/components/transactions/CategorySelect';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';
import TagEditor from './TagEditor';
import DetailActions from './DetailActions';

type Form = ReturnType<typeof useTransactionForm>;

const FIELD_LABEL = 'text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70';

/** Merchant row, showing raw and normalised names. */
function MerchantRow({ form, originalName }: { form: Form; originalName: string }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <Label htmlFor="merchant-name" className={FIELD_LABEL}>
          Merchant Name
        </Label>
        {form.merchant !== originalName && (
          <button
            type="button"
            onClick={() => form.setMerchant(originalName)}
            className="text-[10px] font-semibold text-[#064E3B]/60 hover:text-[#064E3B] underline cursor-pointer"
          >
            Reset
          </button>
        )}
      </div>
      <MerchantField
        id="merchant-name"
        merchant={form.merchant}
        onChange={form.setMerchant}
        onSubmit={form.handleSave}
      />
    </div>
  );
}

/** Transaction metadata: reference, channel, timestamps. */
export default function MetadataCard({
  form,
  originalName,
  onViewSource,
}: {
  form: Form;
  originalName: string;
  onViewSource: () => void;
}) {
  return (
    <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs">
      <CardHeader className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
        <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
          Metadata &amp; Categorization
        </CardTitle>
      </CardHeader>
      <CardContent className="p-5 space-y-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <MerchantRow form={form} originalName={originalName} />

          <div className="space-y-1.5">
            <Label htmlFor="category" className={FIELD_LABEL}>
              Category
            </Label>
            <CategorySelect
              categoryId={form.categoryId}
              onChange={form.setCategoryId}
              categories={form.categories}
              id="category"
              triggerClassName="h-9 text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl w-full"
            />
          </div>
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="notes" className={FIELD_LABEL}>
            Notes
          </Label>
          <Textarea
            id="notes"
            value={form.notes}
            onChange={(e) => form.setNotes(e.target.value)}
            placeholder="Add private notes or annotations…"
            rows={3}
            className="text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl resize-none"
          />
        </div>

        <TagEditor
          tags={form.tags}
          availableTags={form.availableTags}
          newTag={form.newTag}
          setNewTag={form.setNewTag}
          onAddTag={form.handleAddTag}
          onRemoveTag={form.handleRemoveTag}
        />

        {form.showSavedConfirm && (
          <p
            role="status"
            className="flex items-center gap-1.5 text-xs text-emerald-700 font-semibold bg-emerald-500/10 p-2 rounded-lg border border-emerald-500/20"
          >
            <CheckCircle2 className="w-4 h-4 text-emerald-600" aria-hidden="true" />
            Changes saved successfully.
          </p>
        )}

        <DetailActions
          isDirty={form.isDirty}
          isSaving={form.updateFields.isPending}
          isDeleting={form.softDelete.isPending}
          showSavedConfirmation={form.showSavedConfirm}
          onSave={form.handleSave}
          onDelete={form.handleDelete}
          onViewSource={onViewSource}
        />
      </CardContent>
    </Card>
  );
}
