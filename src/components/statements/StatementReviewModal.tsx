import { useCallback, useEffect, useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { useToast } from '@/hooks/use-toast';
import { API, DraftMetadataInput, DraftRow, StatementDraft } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useCommitStatementDraft } from '@/hooks/mutations/useCommitStatementDraft';
import { useDiscardStatementDraft } from '@/hooks/mutations/useDiscardStatementDraft';

function base64ToBlob(base64: string): Blob {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type: 'application/pdf' });
}

const STAGE_LABELS: Record<string, string> = {
  parsing: 'Parsing PDF…',
  metadata: 'Reading statement details…',
  instrument_check: 'Identifying bank and card…',
  duplicate_check: 'Checking for duplicates…',
  extracting_rows: 'Extracting transactions…',
  staged: 'Ready for review',
};

export default function StatementReviewModal() {
  const { toast } = useToast();
  const { reviewModalOpen, activeDraftId, processingProgress, closeReviewModal } = useGlobalState();
  const commitDraft = useCommitStatementDraft();
  const discardDraft = useDiscardStatementDraft();

  const [pdfUrl, setPdfUrl] = useState<string | null>(null);
  const [draft, setDraft] = useState<StatementDraft | null>(null);
  const [metadata, setMetadata] = useState<DraftMetadataInput | null>(null);
  const [rows, setRows] = useState<DraftRow[]>([]);

  const isStaged = processingProgress?.stage === 'staged' || !!draft;

  useEffect(() => {
    if (!reviewModalOpen || !activeDraftId) {
      setPdfUrl(null);
      setDraft(null);
      setMetadata(null);
      setRows([]);
      return;
    }
    let cancelled = false;
    API.statements
      .getDraftPdf(activeDraftId)
      .then((base64) => {
        if (cancelled) return;
        setPdfUrl(URL.createObjectURL(base64ToBlob(base64)));
      })
      .catch(() => {
        if (cancelled) return;
        toast({
          title: 'PDF unavailable',
          description: "This statement's PDF could not be loaded.",
          variant: 'destructive',
        });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reviewModalOpen, activeDraftId]);

  useEffect(() => {
    return () => {
      if (pdfUrl) URL.revokeObjectURL(pdfUrl);
    };
  }, [pdfUrl]);

  useEffect(() => {
    if (!activeDraftId) return;
    if (processingProgress && processingProgress.stage !== 'staged') return;
    let cancelled = false;
    API.statements
      .getDraft(activeDraftId)
      .then((d) => {
        if (cancelled) return;
        setDraft(d);
        setMetadata({
          issuerName: d.issuer_name ?? '',
          maskedIdentifier: d.masked_identifier ?? '',
          instrumentType: d.instrument_type ?? 'credit_card',
          billingPeriodStart: d.billing_period_start,
          billingPeriodEnd: d.billing_period_end,
          dueDate: d.due_date,
          statementDate: d.statement_date,
          currentBalance: d.current_balance,
          minimumDue: d.minimum_due,
        });
        setRows(d.rows);
      })
      .catch(() => {
        if (!cancelled) toast({ title: 'Could not load extracted data', variant: 'destructive' });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDraftId, processingProgress?.stage]);

  const updateRow = useCallback((index: number, field: keyof DraftRow, value: string) => {
    setRows((prev) =>
      prev.map((row, i) => {
        if (i !== index) return row;
        if (field === 'amount_minor')
          return { ...row, amount_minor: Math.round(parseFloat(value || '0') * 100) };
        return { ...row, [field]: value };
      })
    );
  }, []);

  const addRow = useCallback(() => {
    setRows((prev) => [
      ...prev,
      {
        transaction_date: '',
        merchant_raw: '',
        amount_minor: 0,
        currency: 'INR',
        direction: 'debit',
        reference_id: null,
        row_index: prev.length,
        llm_extracted: false,
      },
    ]);
  }, []);

  const removeRow = useCallback((index: number) => {
    setRows((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleCancel = () => {
    if (activeDraftId) {
      discardDraft.mutate(activeDraftId, {
        onSuccess: () =>
          toast({ title: 'Discarded', description: 'Statement removed from review.' }),
      });
    }
    closeReviewModal();
  };

  const handleSubmit = () => {
    if (!activeDraftId || !metadata) return;
    commitDraft.mutate(
      { draftId: activeDraftId, metadata, rows },
      {
        onSuccess: () => {
          toast({
            title: 'Statement saved',
            description: 'Extracted transactions have been added.',
          });
          closeReviewModal();
        },
        onError: () => toast({ title: 'Failed to save', variant: 'destructive' }),
      }
    );
  };

  return (
    <Dialog
      open={reviewModalOpen}
      onOpenChange={(open) => {
        if (!open) handleCancel();
      }}
    >
      <DialogContent
        className="sm:max-w-[1200px] w-[95vw] p-0 overflow-hidden flex flex-col max-h-[92vh] h-[820px]"
        aria-labelledby="review-dialog-title"
      >
        <div className="grid grid-cols-1 md:grid-cols-[1fr_480px] flex-1 min-h-0 h-full">
          <div className="bg-white border-r flex flex-col min-h-0 h-full">
            {pdfUrl ? (
              <iframe src={pdfUrl} className="w-full h-full border-0" title="Statement PDF" />
            ) : (
              <div className="w-full h-full flex items-center justify-center text-sm text-muted-foreground">
                Loading PDF…
              </div>
            )}
          </div>

          <div className="p-6 flex flex-col h-full overflow-y-auto bg-background">
            <DialogHeader className="mb-4">
              <DialogTitle id="review-dialog-title">Statement Processing</DialogTitle>
              <DialogDescription>
                {isStaged
                  ? 'Review and correct the extracted data before saving.'
                  : 'Extracting data from your statement…'}
              </DialogDescription>
            </DialogHeader>

            {!isStaged && (
              <div className="space-y-3 py-8">
                <Progress value={processingProgress?.percent ?? 5} />
                <p className="text-sm text-muted-foreground text-center">
                  {STAGE_LABELS[processingProgress?.stage ?? 'parsing']}
                </p>
              </div>
            )}

            {isStaged && metadata && (
              <div className="flex-1 flex flex-col gap-4 min-h-0">
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <Label htmlFor="rm-bank">Bank name</Label>
                    <Input
                      id="rm-bank"
                      value={metadata.issuerName}
                      onChange={(e) => setMetadata({ ...metadata, issuerName: e.target.value })}
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-card">Card number (last 4)</Label>
                    <Input
                      id="rm-card"
                      value={metadata.maskedIdentifier}
                      onChange={(e) =>
                        setMetadata({ ...metadata, maskedIdentifier: e.target.value })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-due">Due date</Label>
                    <Input
                      id="rm-due"
                      type="date"
                      value={metadata.dueDate ?? ''}
                      onChange={(e) =>
                        setMetadata({ ...metadata, dueDate: e.target.value || null })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-billing-date">Billing date</Label>
                    <Input
                      id="rm-billing-date"
                      type="date"
                      value={metadata.statementDate ?? ''}
                      onChange={(e) =>
                        setMetadata({ ...metadata, statementDate: e.target.value || null })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-period-start">Billing cycle start</Label>
                    <Input
                      id="rm-period-start"
                      type="date"
                      value={metadata.billingPeriodStart ?? ''}
                      onChange={(e) =>
                        setMetadata({ ...metadata, billingPeriodStart: e.target.value || null })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-period-end">Billing cycle end</Label>
                    <Input
                      id="rm-period-end"
                      type="date"
                      value={metadata.billingPeriodEnd ?? ''}
                      onChange={(e) =>
                        setMetadata({ ...metadata, billingPeriodEnd: e.target.value || null })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-balance">Current balance (₹)</Label>
                    <Input
                      id="rm-balance"
                      type="number"
                      value={metadata.currentBalance != null ? metadata.currentBalance / 100 : ''}
                      onChange={(e) =>
                        setMetadata({
                          ...metadata,
                          currentBalance: e.target.value
                            ? Math.round(parseFloat(e.target.value) * 100)
                            : null,
                        })
                      }
                    />
                  </div>
                  <div>
                    <Label htmlFor="rm-min-due">Minimum due (₹)</Label>
                    <Input
                      id="rm-min-due"
                      type="number"
                      value={metadata.minimumDue != null ? metadata.minimumDue / 100 : ''}
                      onChange={(e) =>
                        setMetadata({
                          ...metadata,
                          minimumDue: e.target.value
                            ? Math.round(parseFloat(e.target.value) * 100)
                            : null,
                        })
                      }
                    />
                  </div>
                </div>

                <div className="flex-1 min-h-0 flex flex-col">
                  <div className="flex items-center justify-between mb-2">
                    <Label>Transactions ({rows.length})</Label>
                    <Button variant="outline" size="sm" onClick={addRow}>
                      <Plus className="w-3 h-3 mr-1" aria-hidden="true" /> Add row
                    </Button>
                  </div>
                  <div className="flex-1 overflow-y-auto space-y-2 border rounded-md p-2">
                    {rows.map((row, i) => (
                      <div
                        key={i}
                        className="grid grid-cols-[110px_1fr_90px_70px_28px] gap-1.5 items-center"
                      >
                        <Input
                          type="date"
                          value={row.transaction_date}
                          onChange={(e) => updateRow(i, 'transaction_date', e.target.value)}
                          aria-label={`Row ${i + 1} date`}
                        />
                        <Input
                          value={row.merchant_raw}
                          onChange={(e) => updateRow(i, 'merchant_raw', e.target.value)}
                          aria-label={`Row ${i + 1} merchant`}
                        />
                        <Input
                          type="number"
                          value={row.amount_minor / 100}
                          onChange={(e) => updateRow(i, 'amount_minor', e.target.value)}
                          aria-label={`Row ${i + 1} amount`}
                        />
                        <select
                          className="h-9 rounded-md border border-input bg-background px-2 text-sm"
                          value={row.direction}
                          onChange={(e) => updateRow(i, 'direction', e.target.value)}
                          aria-label={`Row ${i + 1} direction`}
                        >
                          <option value="debit">Debit</option>
                          <option value="credit">Credit</option>
                        </select>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => removeRow(i)}
                          aria-label={`Delete row ${i + 1}`}
                        >
                          <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
                        </Button>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}

            <DialogFooter className="pt-4 grid grid-cols-2 gap-3">
              <Button variant="outline" onClick={handleCancel} disabled={discardDraft.isPending}>
                Cancel
              </Button>
              <Button onClick={handleSubmit} disabled={!isStaged || commitDraft.isPending}>
                {commitDraft.isPending ? 'Saving…' : 'Submit'}
              </Button>
            </DialogFooter>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
