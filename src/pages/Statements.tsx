import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { FileText, Lock, AlertTriangle, FileSearch, CreditCard } from 'lucide-react';
import { API } from '@/lib/ipc';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ScrollArea } from '@/components/ui/scroll-area';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/getErrorMessage';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import { queryKeys } from '@/lib/queryKeys';
import StatementUploadDropzone from '@/components/statements/StatementUploadDropzone';
import UnprocessedItemsQueue from '@/components/statements/UnprocessedItemsQueue';
import PasswordPromptModal from '@/components/statements/PasswordPromptModal';

/**
 * TASK-FE-012 (Doc 30): StatementsList orchestrator — the upload dropzone,
 * unprocessed-items queue, and password modal are now their own named
 * components; this page keeps the processing-history table plus the
 * Statement Instrument Gate confirmation and access-denied modals (real,
 * necessary functionality Doc30 doesn't name a file for, kept inline —
 * same precedent as Dashboard/Instruments preserving un-named real UI).
 */
export default function Statements() {
  const navigate = useNavigate();
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

  const [accessDeniedModalOpen, setAccessDeniedModalOpen] = useState(false);

  useEffect(() => {
    if (instrumentModalOpen) {
      setInstrumentIssuer(pendingInstrumentIssuerHint || '');
      setInstrumentMasked('');
      setInstrumentType('credit_card');
      setInstrumentError(null);
    }
  }, [instrumentModalOpen, pendingInstrumentIssuerHint]);

  const submitInstrumentConfirmation = useCallback(async () => {
    if (!pendingInstrumentStatementId || !instrumentIssuer.trim() || !instrumentMasked.trim()) return;
    setIsSubmittingInstrument(true);
    setInstrumentError(null);
    try {
      await API.statements.confirmInstrument(
        pendingInstrumentStatementId,
        instrumentIssuer.trim(),
        instrumentMasked.trim(),
        instrumentType,
      );
      closeInstrumentModal();
      toast({ title: 'Instrument Confirmed', description: 'Retrying statement extraction…' });
      refresh();
    } catch (e) {
      setInstrumentError(getErrorMessage(e, 'Could not process the statement with these details.'));
    } finally {
      setIsSubmittingInstrument(false);
    }
  }, [pendingInstrumentStatementId, instrumentIssuer, instrumentMasked, instrumentType, closeInstrumentModal, refresh, toast]);

  return (
    <div className="space-y-8 animate-in fade-in duration-500 h-[calc(100vh-80px)] flex flex-col">
      <header>
        <h1 className="text-3xl font-bold tracking-tight">Statements</h1>
        <p className="text-muted-foreground mt-1">Upload and manage your bank statements securely.</p>
      </header>

      <StatementUploadDropzone onUploaded={refresh} onAccessDenied={() => setAccessDeniedModalOpen(true)} />

      <UnprocessedItemsQueue onEnterPassword={(statementId) => openPasswordModal(statementId)} />

      {/* Processing History Table */}
      <Card className="flex-1 flex flex-col min-h-0 border-border/60">
        <CardHeader className="pb-3">
          <CardTitle>Processing History</CardTitle>
          <CardDescription>Recent uploads and their parsing status.</CardDescription>
        </CardHeader>
        <CardContent className="flex-1 p-0 flex flex-col overflow-hidden">
          <ScrollArea className="flex-1">
            <div tabIndex={0} className="min-w-full">
              <Table aria-label="Statement processing history">
                <TableHeader className="sticky top-0 bg-card z-10 border-b border-border/40">
                  <TableRow>
                    <TableHead scope="col">Date</TableHead>
                    <TableHead scope="col">File Name</TableHead>
                    <TableHead scope="col">Status</TableHead>
                    <TableHead scope="col" className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {loading ? (
                    <TableRow>
                      <TableCell colSpan={4} className="text-center h-24" aria-live="polite">
                        Loading history…
                      </TableCell>
                    </TableRow>
                  ) : history.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={4} className="text-center h-24 text-muted-foreground">
                        No statements uploaded yet.
                      </TableCell>
                    </TableRow>
                  ) : (
                    history.map((stmt) => {
                      const isProcessed = stmt.status === 'PROCESSED';
                      const isLocked = stmt.status === 'PASSWORD_REQUIRED';
                      const isOCR = stmt.status === 'OCR_FALLBACK';
                      return (
                        <TableRow key={stmt.id}>
                          <TableCell className="text-muted-foreground w-1/4">
                            {new Date(stmt.date).toLocaleDateString()}
                          </TableCell>
                          <TableCell className="font-medium">
                            <div className="flex items-center gap-2">
                              <FileText className="w-4 h-4 text-muted-foreground shrink-0" aria-hidden="true" />
                              {stmt.file_name}
                            </div>
                          </TableCell>
                          <TableCell className="w-1/4">
                            <Badge
                              variant={isProcessed ? 'default' : isLocked ? 'destructive' : isOCR ? 'secondary' : 'outline'}
                              className="flex w-fit items-center gap-1.5"
                            >
                              {isLocked && <Lock className="w-3 h-3" aria-hidden="true" />}
                              {isOCR && <FileSearch className="w-3 h-3 text-amber-700" aria-hidden="true" />}
                              {stmt.status}
                            </Badge>
                          </TableCell>
                          <TableCell className="text-right">
                            {isProcessed && (
                              <Button
                                variant="outline"
                                size="sm"
                                onClick={() => navigate(stmt.instrument_id ? `/transactions?instrument=${stmt.instrument_id}` : `/transactions?search=${encodeURIComponent(stmt.file_name)}`)}
                              >
                                View Entries
                              </Button>
                            )}
                          </TableCell>
                        </TableRow>
                      );
                    })
                  )}
                </TableBody>
              </Table>
            </div>
          </ScrollArea>
        </CardContent>
      </Card>

      <PasswordPromptModal onUnlocked={refresh} />

      {/* Statement Instrument Gate Confirmation Modal */}
      <Dialog open={instrumentModalOpen} onOpenChange={(open) => { if (!open) closeInstrumentModal(); }}>
        <DialogContent className="sm:max-w-[425px]" aria-labelledby="instrument-dialog-title" aria-describedby="instrument-dialog-desc">
          <DialogHeader>
            <DialogTitle id="instrument-dialog-title" className="flex items-center gap-2">
              <CreditCard className="w-5 h-5 text-amber-700" aria-hidden="true" />
              Confirm Statement Details
            </DialogTitle>
            <DialogDescription id="instrument-dialog-desc">
              {pendingInstrumentFilename && <>{pendingInstrumentFilename}: </>}
              {pendingInstrumentReason || 'We could not automatically identify the issuer or account for this statement.'}
              {' '}Please confirm the details below so we know which account these transactions belong to.
            </DialogDescription>
          </DialogHeader>

          <div className="py-2 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="instrument-issuer">Issuer / Bank Name</Label>
              <Input
                id="instrument-issuer"
                placeholder="e.g. HDFC Bank"
                value={instrumentIssuer}
                onChange={(e) => { setInstrumentIssuer(e.target.value); setInstrumentError(null); }}
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
                onChange={(e) => { setInstrumentMasked(e.target.value.replace(/\D/g, '')); setInstrumentError(null); }}
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
            <Button variant="outline" onClick={closeInstrumentModal} aria-label="Cancel instrument confirmation">
              Cancel
            </Button>
            <Button
              onClick={submitInstrumentConfirmation}
              disabled={!instrumentIssuer.trim() || !instrumentMasked.trim() || isSubmittingInstrument}
              aria-label="Confirm statement instrument details"
            >
              {isSubmittingInstrument ? 'Processing…' : 'Confirm & Continue'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Access Denied Modal */}
      <Dialog open={accessDeniedModalOpen} onOpenChange={setAccessDeniedModalOpen}>
        <DialogContent className="sm:max-w-[500px]" aria-labelledby="access-denied-title" aria-describedby="access-denied-desc">
          <DialogHeader>
            <DialogTitle id="access-denied-title" className="flex items-center gap-2 text-red-700">
              <AlertTriangle className="w-5 h-5" aria-hidden="true" />
              File Access Denied
            </DialogTitle>
            <DialogDescription id="access-denied-desc" className="text-base pt-2">
              macOS blocked access to this file. To fix this, please grant Dinero permission to read files in this
              folder by going to:
            </DialogDescription>
          </DialogHeader>
          <div className="bg-secondary/50 p-4 rounded-md my-2 text-sm font-medium border border-border">
            System Settings {'>'} Privacy &amp; Security {'>'} Files and Folders
          </div>
          <DialogFooter>
            <Button onClick={() => setAccessDeniedModalOpen(false)} aria-label="Dismiss access denied dialog">
              Dismiss
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
