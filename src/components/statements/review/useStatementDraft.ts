/**
 * Draft state for the review dialog: loading, editing, committing, discarding.
 */
import { useCallback, useEffect, useState } from 'react';
import { API, type DraftMetadataInput, type DraftRow, type StatementDraft } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useCommitStatementDraft } from '@/hooks/mutations/useCommitStatementDraft';
import { useDiscardStatementDraft } from '@/hooks/mutations/useDiscardStatementDraft';

/** Converts base64 PDF bytes into a blob. */
function base64ToBlob(base64: string): Blob {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type: 'application/pdf' });
}

/** Projects a draft into editable metadata fields. */
function metadataFromDraft(d: StatementDraft): DraftMetadataInput {
  return {
    issuerName: d.issuer_name ?? '',
    maskedIdentifier: d.masked_identifier ?? '',
    instrumentType: d.instrument_type ?? 'credit_card',
    billingPeriodStart: d.billing_period_start,
    billingPeriodEnd: d.billing_period_end,
    dueDate: d.due_date,
    statementDate: d.statement_date,
    currentBalance: d.current_balance,
    minimumDue: d.minimum_due,
  };
}

const EMPTY_ROW: DraftRow = {
  transaction_date: '',
  merchant_raw: '',
  amount_minor: 0,
  currency: 'INR',
  direction: 'debit',
  reference_id: null,
  row_index: 0,
  llm_extracted: false,
};

/** Draft state: loading, editing, committing, discarding. */
export function useStatementDraft() {
  const { toast } = useToast();
  const { reviewModalOpen, activeDraftId, processingProgress, closeReviewModal } = useGlobalState();
  const commitDraft = useCommitStatementDraft();
  const discardDraft = useDiscardStatementDraft();

  const [pdfUrl, setPdfUrl] = useState<string | null>(null);
  const [draft, setDraft] = useState<StatementDraft | null>(null);
  const [metadata, setMetadata] = useState<DraftMetadataInput | null>(null);
  const [rows, setRows] = useState<DraftRow[]>([]);

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
        if (!cancelled) setPdfUrl(URL.createObjectURL(base64ToBlob(base64)));
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
        setMetadata(metadataFromDraft(d));
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
        if (field === 'amount_minor') {
          return { ...row, amount_minor: Math.round(parseFloat(value || '0') * 100) };
        }
        return { ...row, [field]: value };
      })
    );
  }, []);

  const addRow = useCallback(() => {
    setRows((prev) => [...prev, { ...EMPTY_ROW, row_index: prev.length }]);
  }, []);

  const removeRow = useCallback((index: number) => {
    setRows((prev) => prev.filter((_, i) => i !== index));
  }, []);

  /** Discards the draft without committing. */
  const handleCancel = () => {
    if (activeDraftId) {
      discardDraft.mutate(activeDraftId, {
        onSuccess: () =>
          toast({ title: 'Discarded', description: 'Statement removed from review.' }),
      });
    }
    closeReviewModal();
  };

  /** Commits the reviewed rows into the ledger. */
  const handleSubmit = () => {
    if (!activeDraftId || !metadata) return;
    commitDraft.mutate(
      { draftId: activeDraftId, metadata, rows },
      {
        onSuccess: () => {
          toast({ title: 'Statement saved', description: 'Extracted transactions have been added.' });
          closeReviewModal();
        },
        onError: () => toast({ title: 'Failed to save', variant: 'destructive' }),
      }
    );
  };

  return {
    reviewModalOpen,
    processingProgress,
    pdfUrl,
    metadata,
    setMetadata,
    rows,
    updateRow,
    addRow,
    removeRow,
    isStaged: processingProgress?.stage === 'staged' || !!draft,
    isSaving: commitDraft.isPending,
    isDiscarding: discardDraft.isPending,
    handleCancel,
    handleSubmit,
  };
}
