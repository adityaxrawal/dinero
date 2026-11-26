/**
 * Draft state for the review dialog: loading, editing, committing, discarding.
 */
import { statementCommitToast } from '@/lib/engineOutcome';
import { getErrorToast } from '@/lib/errorMapping';
import { useCallback, useEffect, useState } from 'react';
import { API, type DraftMetadataInput, type DraftRow, type StatementDraft } from '@/lib/ipc';
import { TOAST_AUTO_DISMISS_DELAY, useToast } from '@/hooks/use-toast';
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
  const { reviewModalOpen, activeDraftId, processingProgress, openReviewModal, closeReviewModal } =
    useGlobalState();
  const commitDraft = useCommitStatementDraft();
  const discardDraft = useDiscardStatementDraft();

  const [pdfUrl, setPdfUrl] = useState<string | null>(null);
  const [draft, setDraft] = useState<StatementDraft | null>(null);
  const [metadata, setMetadata] = useState<DraftMetadataInput | null>(null);
  const [rows, setRows] = useState<DraftRow[]>([]);
  // A partial draft (unreadable pages) is saved only once acknowledged.
  const [partialAcknowledged, setPartialAcknowledged] = useState(false);

  useEffect(() => {
    if (!reviewModalOpen || !activeDraftId) {
      setPdfUrl(null);
      setDraft(null);
      setMetadata(null);
      setRows([]);
      setPartialAcknowledged(false);
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

  /**
   * Discards the draft without committing. The delete (draft row and stored
   * PDF) waits until the Undo toast has gone, so Undo just cancels it and
   * reopens the draft (audit F8-6).
   */
  const handleCancel = () => {
    const draftId = activeDraftId;
    closeReviewModal();
    if (!draftId) return;
    const timer = setTimeout(
      () =>
        discardDraft.mutate(draftId, {
          onError: (err) =>
            toast({ variant: 'destructive', ...getErrorToast(err, 'Could not discard') }),
        }),
      TOAST_AUTO_DISMISS_DELAY
    );
    toast({
      title: 'Discarded',
      description: 'Statement removed from review.',
      actionLabel: 'Undo',
      onAction: () => {
        clearTimeout(timer);
        openReviewModal(draftId);
      },
    });
  };

  /** Commits the reviewed rows into the ledger. */
  const handleSubmit = () => {
    if (!activeDraftId || !metadata) return;
    commitDraft.mutate(
      { draftId: activeDraftId, metadata, rows, acknowledgePartial: partialAcknowledged },
      {
        onSuccess: (result) => {
          toast(statementCommitToast(result?.summary));
          closeReviewModal();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err, 'Failed to save') }),
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
    missingPages: draft?.missing_pages ?? [],
    partialAcknowledged,
    setPartialAcknowledged,
    isSaving: commitDraft.isPending,
    isDiscarding: discardDraft.isPending,
    handleCancel,
    handleSubmit,
  };
}
