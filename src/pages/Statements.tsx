import { useCallback, useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import {
  FileText,
  AlertTriangle,
  FileSearch,
  CreditCard,
  ChevronRight,
  Upload,
  Trash2,
} from 'lucide-react';
import { API } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/errorMapping';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import { queryKeys } from '@/lib/queryKeys';
import { cn } from '@/lib/utils';
import { PageSidebar } from '@/components/layout/PageSidebar';
import StatementUploadDropzone from '@/components/statements/StatementUploadDropzone';
import UnprocessedItemsQueue from '@/components/statements/UnprocessedItemsQueue';
import PasswordPromptModal from '@/components/statements/PasswordPromptModal';
import StatementReviewModal from '@/components/statements/StatementReviewModal';
import StatementPdfViewerModal from '@/components/statements/StatementPdfViewerModal';
import { formatCustomDate } from '@/lib/formatCustomDate';

export default function Statements() {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const currentSection = searchParams.get('section') || 'upload';
  const setSection = (section: string) => setSearchParams({ section });
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const {
    openPasswordModal,
    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentFilename,
    pendingInstrumentIssuerHint,
    pendingInstrumentReason,
    closeInstrumentModal,
    openReviewModal,
  } = useGlobalState();

  const { data: history = [], isLoading: loading } = useStatementsList();

  const refresh = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
  }, [queryClient]);

  // Form state for instrument confirmation modal (Statement Instrument Gate)
  const [instrumentIssuer, setInstrumentIssuer] = useState('');
  const [instrumentMasked, setInstrumentMasked] = useState('');
  const [instrumentType, setInstrumentType] = useState('credit_card');
  const [instrumentError, setInstrumentError] = useState<string | null>(null);
  const [isSubmittingInstrument, setIsSubmittingInstrument] = useState(false);
  const [viewingPdfStatementId, setViewingPdfStatementId] = useState<string | null>(null);

  function displayStatementName(stmt: (typeof history)[number]): string {
    if (stmt.issuer_name && stmt.masked_identifier) {
      const typeLabel = stmt.instrument_type === 'bank_account' ? 'Bank Account' : 'Credit Card';
      return `${stmt.issuer_name} ${typeLabel} •••${stmt.masked_identifier}`;
    }
    return stmt.file_name;
  }

  const handleDeletePdf = async (stmt: (typeof history)[number]) => {
    try {
      await API.statements.deletePdf(stmt.id);
      toast({ title: 'PDF deleted', description: 'The stored PDF has been removed.' });
      refresh();
    } catch (e) {
      toast({
        title: 'Could not delete PDF',
        description: getErrorMessage(e, 'Please try again.'),
        variant: 'destructive',
      });
    }
  };

  useEffect(() => {
    if (instrumentModalOpen) {
      setInstrumentIssuer(pendingInstrumentIssuerHint || '');
      setInstrumentMasked('');
      setInstrumentType('credit_card');
      setInstrumentError(null);
    }
  }, [instrumentModalOpen, pendingInstrumentIssuerHint]);

  const submitInstrumentConfirmation = useCallback(async () => {
    if (!pendingInstrumentStatementId || !instrumentIssuer.trim() || !instrumentMasked.trim())
      return;
    setIsSubmittingInstrument(true);
    setInstrumentError(null);
    try {
      // Resumes the same synchronous stage_parse_pipeline as the password
      // path — reuses pendingInstrumentStatementId as the draft id, so the
      // review modal can open directly with it, no event race.
      const result = await API.statements.confirmInstrument(
        pendingInstrumentStatementId,
        instrumentIssuer.trim(),
        instrumentMasked.trim(),
        instrumentType
      );
      closeInstrumentModal();
      openReviewModal(result.draft_id || pendingInstrumentStatementId);
      refresh();
    } catch (e) {
      setInstrumentError(getErrorMessage(e, 'Could not process the statement with these details.'));
    } finally {
      setIsSubmittingInstrument(false);
    }
  }, [
    pendingInstrumentStatementId,
    instrumentIssuer,
    instrumentMasked,
    instrumentType,
    closeInstrumentModal,
    refresh,
    toast,
  ]);

  const SECTIONS = [
    { id: 'upload', label: 'Upload Statements', icon: Upload },
    { id: 'queue', label: 'Action Needed', icon: AlertTriangle },
    { id: 'history', label: 'Processing History', icon: FileSearch },
  ] as const;

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Navigation (Statements) ─────────────────────────────────── */}
      <PageSidebar
        title="Statements"
        sections={SECTIONS}
        currentSection={currentSection as any}
        onSelectSection={setSection}
      />

      {/* ── Column 3: Content Area ────────────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12">
        <div className="max-w-3xl mx-auto space-y-8">
          {/* Zone 1: Upload */}
          {currentSection === 'upload' && (
            <section aria-label="Upload Statements" className="animate-in fade-in duration-300">
              <div className="mb-6">
                <h2 className="text-xl font-bold flex items-center gap-2 text-[#064E3B]">
                  <Upload className="w-5 h-5" /> Upload Statements
                </h2>
                <p className="text-sm mt-1 text-[#064E3B]/70">
                  Drop PDF statements here to parse and extract transactions.
                </p>
              </div>
              <StatementUploadDropzone onUploaded={refresh} />
            </section>
          )}

          {/* Zone 2: Action Needed Queue */}
          {currentSection === 'queue' && (
            <section aria-label="Action Needed" className="animate-in fade-in duration-300">
              <div className="mb-6">
                <h2 className="text-xl font-bold flex items-center gap-2 text-[#064E3B]">
                  <AlertTriangle className="w-5 h-5 text-amber-600" /> Action Needed
                </h2>
                <p className="text-sm mt-1 text-[#064E3B]/70">
                  Statements requiring a password or further attention.
                </p>
              </div>
              <UnprocessedItemsQueue
                onEnterPassword={(statementId) => openPasswordModal(statementId)}
              />
            </section>
          )}

          {/* Zone 3: Processing History */}
          {currentSection === 'history' && (
            <section aria-label="Processing History" className="animate-in fade-in duration-300">
              <div className="mb-6">
                <h2 className="text-xl font-bold flex items-center gap-2 text-[#064E3B]">
                  <FileSearch className="w-5 h-5" /> Processing History
                </h2>
                <p className="text-sm mt-1 text-[#064E3B]/70">
                  Previously parsed and extracted statements.
                </p>
              </div>

              <div className="bg-[#F8E7C9]/50 rounded-xl overflow-hidden border border-[#064E3B]/10 flex flex-col">
                {loading ? (
                  <div className="p-8 text-center text-[13px] text-[#064E3B]/70">
                    Loading history…
                  </div>
                ) : history.length === 0 ? (
                  <div className="p-8 text-center text-[13px] text-[#064E3B]/70">
                    No statements uploaded yet.
                  </div>
                ) : (
                  <div className="divide-y divide-[#064E3B]/5">
                    {history.map((stmt) => {
                      // `statements.parse_status` (src-tauri/migrations/
                      // 20260101000025_statements_source_type_and_checks.sql)
                      // is CHECK-constrained to 'queued' | 'processing' | 'parsed' | 'failed'.
                      // Password-required/OCR-fallback statements live in
                      // `unprocessed_statements` (surfaced separately in the
                      // Action Needed queue) and never reach this table.
                      const isProcessed = stmt.status === 'parsed';
                      const isFailed = stmt.status === 'failed';

                      return (
                        <div
                          key={stmt.id}
                          className="p-4 flex items-center justify-between transition-colors hover:bg-[#064E3B]/5"
                        >
                          <div className="min-w-0 pr-4 flex-1">
                            <div className="flex items-center gap-2 mb-1">
                              <FileText className="w-4 h-4 flex-shrink-0 text-[#064E3B]/70" />
                              <span
                                className="text-[14px] font-semibold truncate text-[#064E3B]"
                                title={displayStatementName(stmt)}
                              >
                                {displayStatementName(stmt)}
                              </span>
                            </div>
                            <div className="flex items-center gap-3">
                              <span className="text-[11px] font-medium text-[#064E3B]/60">
                                {formatCustomDate(stmt.date)}
                              </span>
                              <span
                                className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm flex items-center gap-1 uppercase tracking-wider"
                                style={{
                                  background: isProcessed
                                    ? 'rgba(16,185,129,0.15)'
                                    : isFailed
                                      ? 'rgba(239,68,68,0.15)'
                                      : 'rgba(6,78,59,0.1)',
                                  color: isProcessed ? '#059669' : isFailed ? '#dc2626' : '#064E3B',
                                }}
                              >
                                {isFailed && <AlertTriangle className="w-2.5 h-2.5" />}
                                {stmt.status.replace('_', ' ')}
                              </span>
                            </div>
                          </div>
                          <div className="flex items-center gap-2">
                            {isProcessed && stmt.pdf_available && (
                              <button
                                type="button"
                                className="h-8 px-3 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors bg-[#064E3B]/5 hover:bg-[#064E3B]/10 text-[#064E3B] text-[12px] font-medium gap-1.5"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  setViewingPdfStatementId(stmt.id);
                                }}
                                aria-label="View statement PDF"
                                title="View statement PDF"
                              >
                                <FileText className="w-3.5 h-3.5" />
                                View PDF
                              </button>
                            )}
                            {isProcessed && stmt.pdf_available && (
                              <button
                                type="button"
                                className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors bg-red-50 hover:bg-red-100 text-red-700"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleDeletePdf(stmt);
                                }}
                                aria-label="Delete stored PDF"
                                title="Delete stored PDF"
                              >
                                <Trash2 className="w-3.5 h-3.5" />
                              </button>
                            )}
                            {isProcessed && (
                              <button
                                type="button"
                                className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors bg-[#064E3B]/10 hover:bg-[#064E3B]/20 text-[#064E3B]"
                                onClick={() =>
                                  navigate(
                                    stmt.instrument_id
                                      ? `/transactions?instrument=${stmt.instrument_id}`
                                      : `/transactions?search=${encodeURIComponent(stmt.file_name)}`
                                  )
                                }
                                aria-label="View parsed transactions"
                                title="View parsed transactions"
                              >
                                <ChevronRight className="w-4 h-4" />
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </section>
          )}
        </div>
      </div>

      <PasswordPromptModal onUnlocked={refresh} />

      <StatementReviewModal />

      <StatementPdfViewerModal
        statementId={viewingPdfStatementId}
        open={viewingPdfStatementId !== null}
        onOpenChange={(open) => {
          if (!open) setViewingPdfStatementId(null);
        }}
      />

      {/* Statement Instrument Gate Confirmation Modal */}
      <Dialog
        open={instrumentModalOpen}
        onOpenChange={(open) => {
          if (!open) closeInstrumentModal();
        }}
      >
        <DialogContent
          className="sm:max-w-[425px]"
          aria-labelledby="instrument-dialog-title"
          aria-describedby="instrument-dialog-desc"
        >
          <DialogHeader>
            <DialogTitle id="instrument-dialog-title" className="flex items-center gap-2">
              <CreditCard className="w-5 h-5 text-amber-700" aria-hidden="true" />
              Confirm Statement Details
            </DialogTitle>
            <DialogDescription id="instrument-dialog-desc">
              {pendingInstrumentFilename && <>{pendingInstrumentFilename}: </>}
              {pendingInstrumentReason ||
                'We could not automatically identify the issuer or account for this statement.'}{' '}
              Please confirm the details below so we know which account these transactions belong
              to.
            </DialogDescription>
          </DialogHeader>

          <div className="py-2 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="instrument-issuer">Issuer / Bank Name</Label>
              <Input
                id="instrument-issuer"
                placeholder="e.g. HDFC Bank"
                value={instrumentIssuer}
                onChange={(e) => {
                  setInstrumentIssuer(e.target.value);
                  setInstrumentError(null);
                }}
                autoFocus
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="instrument-masked">Last 4 Digits (Card or Account Number)</Label>
              <Input
                id="instrument-masked"
                placeholder="e.g. 4321"
                maxLength={4}
                value={instrumentMasked}
                onChange={(e) => {
                  setInstrumentMasked(e.target.value.replace(/\D/g, ''));
                  setInstrumentError(null);
                }}
                onKeyDown={(e) => e.key === 'Enter' && submitInstrumentConfirmation()}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="instrument-type">Account Type</Label>
              <Select value={instrumentType} onValueChange={setInstrumentType}>
                <SelectTrigger id="instrument-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="credit_card">Credit Card</SelectItem>
                  <SelectItem value="bank_account">Bank Account</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {instrumentError && (
              <p role="alert" className="text-sm text-red-700 flex items-center gap-1">
                <AlertTriangle className="w-3 h-3" aria-hidden="true" />
                {instrumentError}
              </p>
            )}
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={closeInstrumentModal}
              aria-label="Cancel instrument confirmation"
            >
              Cancel
            </Button>
            <Button
              onClick={submitInstrumentConfirmation}
              disabled={
                !instrumentIssuer.trim() || !instrumentMasked.trim() || isSubmittingInstrument
              }
              aria-label="Confirm statement instrument details"
            >
              {isSubmittingInstrument ? 'Processing…' : 'Confirm & Continue'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
