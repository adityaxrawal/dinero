import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import {
  ArrowLeft,
  Loader2,
  Save,
  Trash2,
  Plus,
  X,
  CheckCircle2,
  ArrowDownLeft,
  ArrowUpRight,
  Repeat,
  MapPin,
  Hash,
  ShieldCheck,
  Link2,
  ChevronDown,
  ChevronUp,
  Pencil,
} from 'lucide-react';
import { TagDatalist } from '@/components/transactions/TagDatalist';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { getErrorToast } from '@/lib/errorMapping';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { useTransactionForm } from '@/components/transactions/useTransactionForm';
import { TransactionAmountBalance } from '@/components/transactions/TransactionAmountBalance';
import { InfoRow } from '@/components/ui/InfoRow';
import { CategorySelect } from '@/components/transactions/CategorySelect';
import { instrumentIcon, instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import { InstrumentSelect } from '@/components/instruments/InstrumentSelect';
import SourceEvidencePanel from '@/components/transactions/SourceEvidencePanel';
import EmiInstallmentTimeline from '@/components/transactions/EmiInstallmentTimeline';

import { formatMoney } from '@/lib/formatMoney';

function JsonViewer({ data }: { data: unknown }) {
  if (typeof data === 'string') return <span className="text-green-400 break-all">"{data}"</span>;
  if (typeof data === 'number') return <span className="text-orange-400">{data}</span>;
  if (typeof data === 'boolean') return <span className="text-purple-400">{data ? 'true' : 'false'}</span>;
  if (data === null || data === undefined) return <span className="text-muted-foreground">null</span>;
  if (Array.isArray(data)) {
    if (data.length === 0) return <span className="text-muted-foreground">[]</span>;
    return (
      <div className="pl-2 border-l border-border/40 ml-2">
        {data.map((item, index) => (
          <div key={index} className="flex">
            <JsonViewer data={item} />
            {index < data.length - 1 && <span className="text-muted-foreground">,</span>}
          </div>
        ))}
      </div>
    );
  }
  const entries = Object.entries(data as Record<string, unknown>);
  return (
    <div className="pl-2 border-l border-border/40 ml-2">
      {entries.map(([key, value], index) => (
        <div key={key} className="flex flex-wrap items-start">
          <span className="text-blue-400 font-medium mr-2">"{key}":</span>
          <JsonViewer data={value} />
          {index < entries.length - 1 && <span className="text-muted-foreground">,</span>}
        </div>
      ))}
    </div>
  );
}

/**
 * TASK-FE-010 (Doc 30): full editable field display (category, merchant
 * display name, tags, notes), SourceEvidencePanel, EmiInstallmentTimeline
 * (only when emi_group_id present). Replaces the TASK-FE-009 placeholder.
 */
export default function TransactionDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const {
    detail,
    isLoading,
    tags,
    availableTags,
    merchant,
    setMerchant,
    categoryId,
    setCategoryId,
    notes,
    setNotes,
    amountStr,
    setAmountStr,
    direction,
    setDirection,
    eventTime,
    setEventTime,
    instrumentId,
    setInstrumentId,
    instruments,
    newTag,
    setNewTag,
    showSavedConfirm: showSavedConfirmation,
    isDirty,
    updateFields,
    softDelete,
    tx,
    amount,
    isDebit,
    instrument,
    category,
    categories,
    isForeignCurrency,
    handleSave,
    handleAddTag,
    handleRemoveTag,
    handleDelete,
  } = useTransactionForm(id, () => navigate('/transactions'));

  const [isSourceDialogOpen, setIsSourceDialogOpen] = useState(false);
  const [isSourceLoading, setIsSourceLoading] = useState(false);
  const [sourceData, setSourceData] = useState<unknown>(null);
  const [isAuditOpen, setIsAuditOpen] = useState(false);

  if (!id || !detail || !tx) return null;

  const handleViewSource = async () => {
    setIsSourceDialogOpen(true);
    setIsSourceLoading(true);
    try {
      const sourceLog = await API.transactions.getSourceLog(id);
      setSourceData(sourceLog || { error: 'No source data found for this transaction.' });
    } catch {
      setSourceData({ error: 'Failed to load source data.' });
    } finally {
      setIsSourceLoading(false);
    }
  };

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center h-40" role="status" aria-label="Loading transaction">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  return (
    // AppLayout's <main> is overflow-hidden, so every routed page owns its
    // own scroll container -- this page's content can exceed the viewport
    // (Correction section + Save/Delete buttons below Provenance), and
    // without this wrapper that overflow was silently clipped with no way
    // to reach it at all. mx-auto centers the fixed max-w-3xl column
    // instead of it pinning to the left edge on a wide window.
    <div className="flex-1 h-full overflow-y-auto">
      <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl mx-auto p-6 lg:p-10">
        <Button variant="ghost" size="sm" onClick={() => navigate('/transactions')} aria-label="Back to transactions">
          <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
        </Button>

        <div className="text-center">
          <div className="flex items-center justify-center gap-1 mb-2">
            <span className={cn('text-3xl font-extrabold font-mono', isDebit ? 'text-red-700' : 'text-emerald-700')}>
              {isDebit ? '−' : '+'}₹
            </span>
            <div className="relative flex items-center group">
              <input
                type="number"
                step="0.01"
                value={amountStr}
                onChange={(e) => setAmountStr(e.target.value)}
                aria-label="Transaction Amount"
                className={cn(
                  'bg-transparent outline-none border-b-2 border-dashed border-current focus:border-solid text-3xl font-extrabold font-mono text-center [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none pr-6',
                  isDebit ? 'text-red-700' : 'text-emerald-700'
                )}
                style={{
                  width: `${Math.max(amountStr.length * 18 + 28, 90)}px`,
                }}
              />
              <Pencil
                className={cn(
                  'w-3.5 h-3.5 opacity-70 group-hover:opacity-100 transition-opacity absolute right-0 pointer-events-none',
                  isDebit ? 'text-red-700' : 'text-emerald-700'
                )}
              />
            </div>
          </div>
          <p className="text-lg font-medium">{tx.merchant_display_name}</p>
          {tx.best_event_time && (
            <p className="text-sm text-muted-foreground">
              {formatCustomDate(tx.best_event_time)}
            </p>
          )}
          <div className="flex items-center justify-center gap-2 flex-wrap mt-3">
            <button
              type="button"
              onClick={() => setDirection(isDebit ? 'credit' : 'debit')}
              className={cn(
                'inline-flex items-center gap-1 px-3 py-1 rounded-full text-xs font-semibold uppercase transition-opacity cursor-pointer hover:opacity-80 border',
                isDebit ? 'text-red-700 bg-red-500/10 border-red-500/30' : 'text-emerald-700 bg-emerald-500/10 border-emerald-500/30'
              )}
              title="Click to toggle Debit / Credit"
            >
              {isDebit ? <ArrowUpRight className="w-3.5 h-3.5" /> : <ArrowDownLeft className="w-3.5 h-3.5" />}
              {isDebit ? 'Debit' : 'Credit'}
            </button>
            {category && (
              <Badge variant="outline" className="flex items-center gap-1.5">
                <span className="w-2 h-2 rounded-full" style={{ background: category.color ?? '#064E3B' }} aria-hidden="true" />
                {category.name}
              </Badge>
            )}
            {tx.transaction_subtype && (
              <Badge variant="outline" className="flex items-center gap-1">
                <Repeat className="w-3 h-3" />
                {tx.transaction_subtype}
              </Badge>
            )}
            {tx.status && <Badge variant="outline">{tx.status}</Badge>}
          </div>
        </div>

        <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs">
          <CardHeader className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
            <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
              Metadata &amp; Categorization
            </CardTitle>
          </CardHeader>
          <CardContent className="p-5 space-y-4">
            {/* 2-Column Grid for Merchant Name & Category */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              {/* Merchant Name */}
              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <Label htmlFor="merchant-name" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                    Merchant Name
                  </Label>
                  {merchant !== (tx.merchant_display_name ?? '') && (
                    <button
                      type="button"
                      onClick={() => setMerchant(tx.merchant_display_name ?? '')}
                      className="text-[10px] font-semibold text-[#064E3B]/60 hover:text-[#064E3B] underline cursor-pointer"
                    >
                      Reset
                    </button>
                  )}
                </div>
                <div className="relative">
                  <Input
                    id="merchant-name"
                    value={merchant}
                    onChange={(e) => setMerchant(e.target.value)}
                    placeholder="Merchant name…"
                    className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl pr-8"
                    onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                  />
                  {merchant && (
                    <button
                      type="button"
                      onClick={() => setMerchant('')}
                      className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/40 hover:text-[#064E3B]"
                      aria-label="Clear merchant name"
                    >
                      <X className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              </div>

              {/* Category Selection */}
              <div className="space-y-1.5">
                <Label htmlFor="category" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                  Category
                </Label>
                <CategorySelect
                  categoryId={categoryId}
                  onChange={setCategoryId}
                  categories={categories}
                  id="category"
                  triggerClassName="h-9 text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl w-full"
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="notes" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                Notes
              </Label>
              <Textarea
                id="notes"
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder="Add private notes or annotations…"
                rows={3}
                className="text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl resize-none"
              />
            </div>

            <div className="space-y-2 pt-1 border-t border-[#064E3B]/10">
              <div className="flex items-center justify-between">
                <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                  Tags
                </Label>
                <span className="text-[10px] text-[#064E3B]/50 font-mono">
                  {tags.length} tag{tags.length !== 1 ? 's' : ''}
                </span>
              </div>
              <div className="flex flex-wrap gap-1.5 min-h-[30px] items-center">
                {tags.length === 0 ? (
                  <span className="text-[12px] italic text-[#064E3B]/40">No tags added yet.</span>
                ) : (
                  tags.map((tag) => (
                    <Badge
                      key={tag}
                      variant="secondary"
                      className="flex items-center gap-1 bg-[#064E3B]/10 text-[#064E3B] hover:bg-[#064E3B]/20 rounded-full px-2.5 py-0.5"
                    >
                      {tag}
                      <button
                        type="button"
                        aria-label={`Remove tag ${tag}`}
                        className="hover:bg-[#064E3B]/20 p-0.5 rounded-full cursor-pointer"
                        onClick={() => handleRemoveTag(tag)}
                      >
                        <X className="w-3 h-3" aria-hidden="true" />
                      </button>
                    </Badge>
                  ))
                )}
              </div>
              <div className="flex gap-2 pt-1">
                <Input
                  aria-label="New tag"
                  placeholder="Add new tag..."
                  list="tag-suggestions"
                  value={newTag}
                  onChange={(e) => setNewTag(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), handleAddTag())}
                  className="h-8 text-[12px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] flex-1"
                />
                <TagDatalist id="tag-suggestions" tags={tags} availableTags={availableTags} />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleAddTag}
                  className="h-8 px-3 border-[#064E3B]/15 text-[#064E3B]"
                >
                  <Plus className="w-3.5 h-3.5 mr-1" aria-hidden="true" /> Add
                </Button>
              </div>
            </div>

            {showSavedConfirmation && (
              <p role="status" className="flex items-center gap-1.5 text-xs text-emerald-700 font-semibold bg-emerald-500/10 p-2 rounded-lg border border-emerald-500/20">
                <CheckCircle2 className="w-4 h-4 text-emerald-600" aria-hidden="true" />
                Changes saved successfully.
              </p>
            )}

            <div className="flex flex-col gap-2 pt-2">
              <Button
                className={cn(
                  'w-full h-9 font-bold rounded-xl transition-all',
                  isDirty
                    ? 'bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-md ring-2 ring-[#064E3B]/30'
                    : 'bg-[#064E3B]/40 text-[#F8E7C9]/70 cursor-not-allowed'
                )}
                onClick={handleSave}
                disabled={updateFields.isPending || (!isDirty && !showSavedConfirmation)}
              >
                {updateFields.isPending ? 'Saving...' : <><Save className="w-4 h-4 mr-2" /> Save Changes</>}
              </Button>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  className="flex-1 h-9 text-red-700 border-red-500/20 bg-red-500/10 hover:bg-red-500/20 font-semibold rounded-xl"
                  onClick={handleDelete}
                  disabled={softDelete.isPending}
                >
                  <Trash2 className="w-4 h-4 mr-2" /> {softDelete.isPending ? 'Deleting...' : 'Delete Transaction'}
                </Button>
                <Button
                  variant="outline"
                  className="flex-1 h-9 border-[#064E3B]/20 hover:bg-[#064E3B]/10 text-[#064E3B] font-semibold rounded-xl"
                  onClick={handleViewSource}
                >
                  View Raw Source
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs">
          <CardHeader className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
            <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
              Payment Instrument &amp; Balance
            </CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            <InstrumentSelect
              instrumentId={instrumentId}
              onInstrumentChange={setInstrumentId}
              instruments={instruments}
            />
            {(tx.balance_after_transaction !== null || isForeignCurrency) && (
              <TransactionAmountBalance tx={tx} isForeignCurrency={isForeignCurrency} />
            )}
          </CardContent>
        </Card>

        <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs overflow-hidden">
          <CardHeader
            className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03] flex flex-row items-center justify-between cursor-pointer select-none hover:bg-[#064E3B]/[0.06] transition-colors"
            onClick={() => setIsAuditOpen((prev) => !prev)}
          >
            <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
              Audit &amp; Technical Specs
            </CardTitle>
            <span className="text-[#064E3B]/60">
              {isAuditOpen ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
            </span>
          </CardHeader>
          {isAuditOpen && (
            <CardContent className="p-0 animate-in fade-in duration-150">
              <InfoRow label="Status">
                <span
                  className="px-2.5 py-0.5 text-[11px] font-bold rounded-full uppercase tracking-wider"
                  style={{
                    background:
                      (tx.status ?? '').toLowerCase() === 'posted'
                        ? 'rgba(16,185,129,0.15)'
                        : 'rgba(107,138,127,0.15)',
                    color: (tx.status ?? '').toLowerCase() === 'posted' ? '#059669' : '#064E3B',
                  }}
                >
                  {tx.status ?? 'UNKNOWN'}
                </span>
              </InfoRow>

              {tx.best_posting_date && <InfoRow label="Posting Date">{tx.best_posting_date}</InfoRow>}

              {tx.reference_id && (
                <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Reference ID" copyValue={tx.reference_id}>
                  <span className="font-mono text-xs">{tx.reference_id}</span>
                </InfoRow>
              )}

              <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Transaction ID" copyValue={tx.id}>
                <span className="font-mono text-xs opacity-90">{tx.id}</span>
              </InfoRow>

              {tx.location && (
                <InfoRow icon={<MapPin className="w-3.5 h-3.5" />} label="Location">
                  {tx.location}
                </InfoRow>
              )}

              {tx.source_mix && (
                <InfoRow icon={<Link2 className="w-3.5 h-3.5" />} label="Source Pipeline">
                  <span className="font-mono text-[11px] uppercase bg-[#064E3B]/5 px-2 py-0.5 rounded border border-[#064E3B]/10">
                    {tx.source_mix}
                  </span>
                </InfoRow>
              )}
              {tx.match_confidence && (
                <InfoRow icon={<ShieldCheck className="w-3.5 h-3.5" />} label="Match Confidence">
                  <span className="capitalize">{tx.match_confidence}</span>
                </InfoRow>
              )}
              {tx.event_time_confidence && (
                <InfoRow label="Time Confidence">
                  <span className="capitalize">{tx.event_time_confidence}</span>
                </InfoRow>
              )}
              {tx.alert_fired !== null && (
                <InfoRow label="Alert Sent">
                  {tx.alert_fired ? 'Yes' : 'No'}
                </InfoRow>
              )}
            </CardContent>
          )}
        </Card>

        <SourceEvidencePanel transactionId={id!} observations={detail.observations} />

        {tx.emi_group_id && <EmiInstallmentTimeline emiGroupId={tx.emi_group_id} />}

        {(tx.created_at || tx.updated_at) && (
          <p className="text-xs text-muted-foreground text-center font-mono opacity-80">
            {tx.created_at && `Recorded ${formatCustomDate(tx.created_at)}`}
            {tx.updated_at && tx.updated_at !== tx.created_at && ` · Updated ${formatCustomDate(tx.updated_at)}`}
          </p>
        )}

        <Dialog open={isSourceDialogOpen} onOpenChange={setIsSourceDialogOpen}>
          <DialogContent className="max-w-3xl max-h-[80vh] flex flex-col">
            <DialogHeader>
              <DialogTitle>Transaction Source Data</DialogTitle>
              <DialogDescription>Exact email/statement data parsed for this transaction.</DialogDescription>
            </DialogHeader>
            <ScrollArea className="flex-1 mt-4 p-4 bg-black/40 rounded-md font-mono text-sm">
              {isSourceLoading ? 'Loading...' : sourceData ? (
                <div className="py-2">
                  {typeof sourceData === 'string' ? (
                    <pre className="whitespace-pre-wrap">{sourceData}</pre>
                  ) : (
                    <JsonViewer data={sourceData} />
                  )}
                </div>
              ) : (
                'No data'
              )}
            </ScrollArea>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  );
}

