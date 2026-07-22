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
import { useTransactionDetail } from '@/hooks/queries/useTransactionDetail';
import { useTransactionTags } from '@/hooks/queries/useTransactionTags';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useTagsList } from '@/hooks/queries/useTagsList';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useUpdateTransactionFields } from '@/hooks/mutations/useUpdateTransactionFields';
import { useAddTransactionTag } from '@/hooks/mutations/useAddTransactionTag';
import { useRemoveTransactionTag } from '@/hooks/mutations/useRemoveTransactionTag';
import { useSoftDeleteTransaction } from '@/hooks/mutations/useSoftDeleteTransaction';
import { confirmDeleteTransaction } from '@/lib/confirmDialog';
import { instrumentIcon, instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import SourceEvidencePanel from '@/components/transactions/SourceEvidencePanel';
import EmiInstallmentTimeline from '@/components/transactions/EmiInstallmentTimeline';

/** A single label/value row inside an info card. */
function InfoRow({
  icon,
  label,
  children,
}: {
  icon?: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2.5 border-b border-border/40 last:border-0">
      <span className="flex items-center gap-1.5 text-sm font-medium text-muted-foreground shrink-0">
        {icon}
        {label}
      </span>
      <span className="text-sm font-medium truncate max-w-[60%] text-right">{children}</span>
    </div>
  );
}

function formatMoney(amountMinor: number | null, currency: string | null) {
  if (amountMinor === null) return '—';
  const symbol = currency === 'USD' ? '$' : currency === 'EUR' ? '€' : currency === 'GBP' ? '£' : currency ? `${currency} ` : '₹';
  return `${symbol}${(amountMinor / 100).toLocaleString(undefined, { minimumFractionDigits: 2 })}`;
}

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
  const { toast } = useToast();

  const { data: detail, isLoading } = useTransactionDetail(id);
  const { data: tags = [] } = useTransactionTags(id);
  const { data: categories = [] } = useCategoriesList();
  const { data: availableTags = [] } = useTagsList();
  const { data: instruments = [] } = useInstrumentsList();

  const updateFields = useUpdateTransactionFields();
  const addTag = useAddTransactionTag();
  const removeTag = useRemoveTransactionTag();
  const softDelete = useSoftDeleteTransaction();

  const [merchant, setMerchant] = useState('');
  const [categoryId, setCategoryId] = useState('');
  const [notes, setNotes] = useState('');
  const [newTag, setNewTag] = useState('');
  // TASK-FE-010: "Thanks, we'll remember this" confirmation tied to the
  // feedback-log learning loop (transactions_update already writes merchant
  // corrections into merchant_aliases server-side, per TASK-API-003).
  const [showSavedConfirmation, setShowSavedConfirmation] = useState(false);

  const [isSourceDialogOpen, setIsSourceDialogOpen] = useState(false);
  const [isSourceLoading, setIsSourceLoading] = useState(false);
  const [sourceData, setSourceData] = useState<unknown>(null);

  useEffect(() => {
    if (detail) {
      setMerchant(detail.transaction.merchant_display_name ?? '');
      setCategoryId(detail.transaction.category_id ?? '');
      setNotes(detail.transaction.notes ?? '');
    }
  }, [detail]);

  if (!id) return null;

  const handleSave = () => {
    updateFields.mutate(
      { transactionId: id, merchantDisplayName: merchant, categoryId, notes },
      {
        onSuccess: () => {
          setShowSavedConfirmation(true);
          setTimeout(() => setShowSavedConfirmation(false), 3000);
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      },
    );
  };

  const handleAddTag = () => {
    const tagName = newTag.trim();
    if (!tagName || tags.includes(tagName)) return;
    addTag.mutate(
      { transactionId: id, tagName },
      { onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }) },
    );
    setNewTag('');
  };

  const handleRemoveTag = (tagName: string) => {
    removeTag.mutate(
      { transactionId: id, tagName },
      { onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }) },
    );
  };

  const handleDelete = async () => {
    const confirmed = await confirmDeleteTransaction();
    if (!confirmed) return;
    softDelete.mutate(id, {
      onSuccess: () => {
        toast({ title: 'Transaction Deleted' });
        navigate('/transactions');
      },
      onError: (err) =>
        toast({ variant: 'destructive', ...getErrorToast(err, 'Only manually-entered transactions can be deleted.') }),
    });
  };

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

  if (isLoading || !detail) {
    return (
      <div className="flex items-center justify-center h-40" role="status" aria-label="Loading transaction">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  const tx = detail.transaction;
  const amount = tx.amount ?? (tx.amount_minor ?? 0) / 100;
  const isDebit = tx.direction === 'debit';
  const instrument = tx.instrument_id ? instruments.find((i) => i.id === tx.instrument_id) : undefined;
  const category = categories.find((c) => c.id === tx.category_id);
  const isForeignCurrency = !!tx.original_amount_minor && tx.original_currency && tx.original_currency !== tx.currency;

  return (
    <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl">
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
        {isForeignCurrency && (
          <p className="text-xs text-muted-foreground mt-1">
            {formatMoney(tx.original_amount_minor, tx.original_currency)}
            {tx.exchange_rate ? ` · rate ${tx.exchange_rate.toFixed(4)}` : ''}
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
            <InfoRow label="Currency">{tx.currency ?? 'INR'}</InfoRow>
            {isForeignCurrency && (
              <>
                <InfoRow label="Original Amount">{formatMoney(tx.original_amount_minor, tx.original_currency)}</InfoRow>
                {tx.exchange_rate !== null && <InfoRow label="Exchange Rate">{tx.exchange_rate?.toFixed(4)}</InfoRow>}
              </>
            )}
            {tx.balance_after_transaction !== null && (
              <InfoRow label="Balance After">
                ₹{tx.balance_after_transaction?.toLocaleString(undefined, { minimumFractionDigits: 2 })}
              </InfoRow>
            )}
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
            <Select value={categoryId} onValueChange={setCategoryId}>
              <SelectTrigger id="category" aria-label="Category">
                <SelectValue placeholder="Select category" />
              </SelectTrigger>
              <SelectContent>
                {categories.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    <span className="flex items-center gap-2">
                      <span className="w-2 h-2 rounded-full shrink-0" style={{ background: c.color ?? '#064E3B' }} aria-hidden="true" />
                      {c.name}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
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
  );
}
