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
    newTag,
    setNewTag,
    showSavedConfirm: showSavedConfirmation,
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
          <h1 className={cn('text-3xl font-bold mb-2', isDebit ? 'text-red-700' : 'text-emerald-700')}>
            {isDebit ? '- ' : '+ '}₹{Math.abs(amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
          </h1>
          <p className="text-lg font-medium">{tx.merchant_display_name}</p>
          {tx.best_event_time && (
            <p className="text-sm text-muted-foreground">
              {formatCustomDate(tx.best_event_time)} {new Date(tx.best_event_time).toLocaleTimeString()}
            </p>
          )}
          <div className="flex items-center justify-center gap-2 flex-wrap mt-3">
            <Badge
              variant="outline"
              className={cn('flex items-center gap-1', isDebit ? 'text-red-700 border-red-700/30' : 'text-emerald-700 border-emerald-700/30')}
            >
              {isDebit ? <ArrowUpRight className="w-3 h-3" /> : <ArrowDownLeft className="w-3 h-3" />}
              {isDebit ? 'Debit' : 'Credit'}
            </Badge>
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

        <Card>
          <CardHeader>
            <CardTitle>Transaction Info</CardTitle>
          </CardHeader>
          <CardContent>
            {instrument && (
              <InfoRow icon={instrumentIcon(instrument.instrument_type, 14)} label="Instrument">
                <span className="flex flex-col items-end">
                  <span>{instrument.issuer_name}</span>
                  <span className="text-xs text-muted-foreground font-normal">
                    {instrumentTypeLabel(instrument.instrument_type)}
                    {instrument.masked_identifier ? ` · ${instrument.masked_identifier}` : ''}
                  </span>
                </span>
              </InfoRow>
            )}
            {tx.reference_id && (
              <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Reference ID">
                <span className="font-mono">{tx.reference_id}</span>
              </InfoRow>
            )}
            {tx.best_posting_date && <InfoRow label="Posting Date">{tx.best_posting_date}</InfoRow>}
            {tx.location && (
              <InfoRow icon={<MapPin className="w-3.5 h-3.5" />} label="Location">
                {tx.location}
              </InfoRow>
            )}
            <InfoRow icon={<Hash className="w-3.5 h-3.5" />} label="Transaction ID">
              <span className="font-mono text-xs">{tx.id}</span>
            </InfoRow>
          </CardContent>
        </Card>

        {(tx.balance_after_transaction !== null || isForeignCurrency) && (
          <Card>
            <CardHeader>
              <CardTitle>Amount &amp; Balance</CardTitle>
            </CardHeader>
            <CardContent>
              <TransactionAmountBalance tx={tx} isForeignCurrency={isForeignCurrency} />
            </CardContent>
          </Card>
        )}

        {(tx.source_mix || tx.match_confidence || tx.event_time_confidence || tx.alert_fired !== null) && (
          <Card>
            <CardHeader>
              <CardTitle>Provenance</CardTitle>
            </CardHeader>
            <CardContent>
              {tx.source_mix && (
                <InfoRow icon={<Link2 className="w-3.5 h-3.5" />} label="Source">
                  {tx.source_mix}
                </InfoRow>
              )}
              {tx.match_confidence && (
                <InfoRow icon={<ShieldCheck className="w-3.5 h-3.5" />} label="Match Confidence">
                  {tx.match_confidence}
                </InfoRow>
              )}
              {tx.event_time_confidence && <InfoRow label="Time Confidence">{tx.event_time_confidence}</InfoRow>}
              {tx.alert_fired !== null && <InfoRow label="Alert Sent">{tx.alert_fired ? 'Yes' : 'No'}</InfoRow>}
            </CardContent>
          </Card>
        )}

        <Card>
          <CardHeader>
            <CardTitle>Correction</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="merchant-name">Merchant Name</Label>
              <Input id="merchant-name" value={merchant} onChange={(e) => setMerchant(e.target.value)} />
            </div>

            <div className="space-y-2">
              <Label htmlFor="category">Category</Label>
              <CategorySelect
                categoryId={categoryId}
                onChange={setCategoryId}
                categories={categories}
                id="category"
                triggerClassName="w-full bg-[#F3EBDD]/50 backdrop-blur-sm border-[#064E3B]/20 focus:ring-[#064E3B]/30"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="notes">Notes</Label>
              <Textarea id="notes" value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Add a note…" rows={3} />
            </div>

            <div className="space-y-2">
              <Label>Tags</Label>
              <div className="flex flex-wrap gap-2 mb-2">
                {tags.map((tag) => (
                  <Badge key={tag} variant="secondary" className="flex items-center gap-1">
                    {tag}
                    <div
                      role="button"
                      aria-label={`Remove tag ${tag}`}
                      className="hover:bg-accent hover:text-accent-foreground w-3 h-3 flex items-center justify-center rounded-sm cursor-pointer"
                      onClick={() => handleRemoveTag(tag)}
                    >
                      <X className="w-3 h-3" aria-hidden="true" />
                    </div>
                  </Badge>
                ))}
              </div>
              <div className="flex gap-2">
                <Input
                  aria-label="New tag"
                  placeholder="New tag..."
                  list="tag-suggestions"
                  value={newTag}
                  onChange={(e) => setNewTag(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && handleAddTag()}
                />
                <TagDatalist id="tag-suggestions" tags={tags} availableTags={availableTags} />
                <Button variant="outline" size="icon" onClick={handleAddTag} aria-label="Add tag">
                  <Plus className="w-4 h-4" aria-hidden="true" />
                </Button>
              </div>
            </div>

            {showSavedConfirmation && (
              <p role="status" className="flex items-center gap-1.5 text-sm text-emerald-700 font-medium">
                <CheckCircle2 className="w-4 h-4" aria-hidden="true" />
                Thanks, we'll remember this.
              </p>
            )}

            <Button className="w-full mt-2" onClick={handleSave} disabled={updateFields.isPending}>
              {updateFields.isPending ? 'Saving...' : <><Save className="w-4 h-4 mr-2" /> Save Corrections</>}
            </Button>
            <Button
              variant="outline"
              className="w-full text-red-700 hover:text-red-700"
              onClick={handleDelete}
              disabled={softDelete.isPending}
            >
              <Trash2 className="w-4 h-4 mr-2" /> {softDelete.isPending ? 'Deleting...' : 'Delete Transaction'}
            </Button>
            <Button variant="outline" className="w-full" onClick={handleViewSource}>
              View Raw Source
            </Button>
          </CardContent>
        </Card>

        <SourceEvidencePanel transactionId={id!} observations={detail.observations} />

        {tx.emi_group_id && <EmiInstallmentTimeline emiGroupId={tx.emi_group_id} />}

        {(tx.created_at || tx.updated_at) && (
          <p className="text-xs text-muted-foreground text-center">
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
