import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2, Save, Trash2, Plus, X, CheckCircle2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { getErrorMessage } from '@/lib/getErrorMessage';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { useTransactionDetail } from '@/hooks/queries/useTransactionDetail';
import { useTransactionTags } from '@/hooks/queries/useTransactionTags';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useTagsList } from '@/hooks/queries/useTagsList';
import { useUpdateTransactionFields } from '@/hooks/mutations/useUpdateTransactionFields';
import { useAddTransactionTag } from '@/hooks/mutations/useAddTransactionTag';
import { useRemoveTransactionTag } from '@/hooks/mutations/useRemoveTransactionTag';
import { useSoftDeleteTransaction } from '@/hooks/mutations/useSoftDeleteTransaction';
import SourceEvidencePanel from '@/components/transactions/SourceEvidencePanel';
import EmiInstallmentTimeline from '@/components/transactions/EmiInstallmentTimeline';

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
        onError: (err) => toast({ variant: 'destructive', title: 'Update failed', description: getErrorMessage(err) }),
      },
    );
  };

  const handleAddTag = () => {
    const tagName = newTag.trim();
    if (!tagName || tags.includes(tagName)) return;
    addTag.mutate(
      { transactionId: id, tagName },
      { onError: (err) => toast({ variant: 'destructive', title: 'Add tag failed', description: getErrorMessage(err) }) },
    );
    setNewTag('');
  };

  const handleRemoveTag = (tagName: string) => {
    removeTag.mutate(
      { transactionId: id, tagName },
      { onError: (err) => toast({ variant: 'destructive', title: 'Remove tag failed', description: getErrorMessage(err) }) },
    );
  };

  const handleDelete = async () => {
    let confirmed: boolean;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask('Delete this transaction? This cannot be undone.', { title: 'Delete Transaction', kind: 'warning' });
    } catch {
      confirmed = confirm('Delete this transaction? This cannot be undone.');
    }
    if (!confirmed) return;
    softDelete.mutate(id, {
      onSuccess: () => {
        toast({ title: 'Transaction Deleted' });
        navigate('/transactions');
      },
      onError: (err) =>
        toast({ variant: 'destructive', title: 'Delete Failed', description: getErrorMessage(err, 'Only manually-entered transactions can be deleted.') }),
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

  return (
    <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl">
      <Button variant="ghost" size="sm" onClick={() => navigate('/transactions')} aria-label="Back to transactions">
        <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
      </Button>

      <div className="text-center">
        <h1 className={cn('text-3xl font-bold mb-2', amount < 0 ? 'text-red-700' : 'text-emerald-700')}>
          {amount < 0 ? '- ' : '+ '}₹{Math.abs(amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
        </h1>
        <p className="text-lg font-medium">{tx.merchant_display_name}</p>
        {tx.best_event_time && (
          <p className="text-sm text-muted-foreground">
            {formatCustomDate(tx.best_event_time)} {new Date(tx.best_event_time).toLocaleTimeString()}
          </p>
        )}
      </div>

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
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label htmlFor="notes">Notes</Label>
            <Input id="notes" value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Add a note…" />
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
              <datalist id="tag-suggestions">
                {availableTags.filter((t) => !tags.includes(t.name)).map((t) => (
                  <option key={t.id} value={t.name} />
                ))}
              </datalist>
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

      <SourceEvidencePanel observations={detail.observations} />

      {tx.emi_group_id && <EmiInstallmentTimeline emiGroupId={tx.emi_group_id} />}

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
