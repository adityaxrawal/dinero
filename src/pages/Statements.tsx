import { useState, useCallback, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { open } from '@tauri-apps/plugin-dialog';
import { UploadCloud, FileText, Lock, AlertTriangle, FileSearch, RefreshCw, Clock, CreditCard } from 'lucide-react';
import { API } from '../lib/ipc';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import { useGlobalState } from '../lib/GlobalStateContext';

function formatCountdown(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

export default function Statements() {
  const navigate = useNavigate();
  const { toast } = useToast();
  const {
    statementHistory: history,
    statementLoading: loading,
    loadStatementHistory: loadHistory,
    passwordModalOpen,
    pendingStatementId,
    pendingInstrumentId,
    passwordTimeoutCountdown: countdown,
    closePasswordModal,
    openPasswordModal,
    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentFilename,
    pendingInstrumentIssuerHint,
    pendingInstrumentReason,
    closeInstrumentModal,
  } = useGlobalState();

  const [isDragging, setIsDragging] = useState(false);

  // Form state for password modal
  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [isSubmittingPassword, setIsSubmittingPassword] = useState(false);

  // Form state for instrument confirmation modal (C2)
  const [instrumentIssuer, setInstrumentIssuer] = useState('');
  const [instrumentMasked, setInstrumentMasked] = useState('');
  const [instrumentType, setInstrumentType] = useState('credit_card');
  const [instrumentError, setInstrumentError] = useState<string | null>(null);
  const [isSubmittingInstrument, setIsSubmittingInstrument] = useState(false);

  // Access denied modal
  const [accessDeniedModalOpen, setAccessDeniedModalOpen] = useState(false);

  // --- File upload ---
  // G8/H2 fix: a single statements_upload call now processes every
  // selected/dropped file as a real batch (Doc 19 §9.1, FR-031) — previously
  // only a single path was accepted and multi-file drops silently kept only
  // the first file.
  const uploadFiles = useCallback(
    async (paths: string[]) => {
      let accessDenied = false;
      let succeeded = 0;
      const failures: string[] = [];

      try {
        const results = await API.statements.upload(paths);
        for (const result of results) {
          // Doc 30 TASK-API-004 fix: the real `UploadResult.status` on
          // failure is `"error: <message>"` (the message is embedded in
          // the same field, there is no separate `error` field on the
          // backend struct) -- the previous exact `=== 'error'` check
          // could never match a real failure, silently counting every
          // failed upload as a success.
          if (result.status.startsWith('error')) {
            const errMsg = result.status.replace(/^error:\s*/, '');
            if (errMsg.includes('File access denied') || errMsg.includes('Permission denied')) {
              accessDenied = true;
            } else {
              failures.push(errMsg);
            }
          } else {
            succeeded += 1;
          }
        }
      } catch (err: any) {
        failures.push(err?.message || String(err));
      }

      if (accessDenied) {
        setAccessDeniedModalOpen(true);
      }
      if (succeeded > 0) {
        toast({
          title: paths.length > 1 ? `${succeeded} of ${paths.length} Uploads Started` : 'Upload Started',
          description: 'Statement(s) are being processed.',
        });
      }
      if (failures.length > 0) {
        toast({
          variant: 'destructive',
          title: 'Some Uploads Failed',
          description: failures.slice(0, 3).join('; '),
        });
      }
      loadHistory();
    },
    [loadHistory, toast],
  );

  const handleFileUpload = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: 'PDF', extensions: ['pdf'] }],
      });

      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length > 0) {
        await uploadFiles(paths);
      }
    } catch (err: any) {
      toast({
        variant: 'destructive',
        title: 'Upload Failed',
        description: err?.message || String(err),
      });
    }
  }, [uploadFiles, toast]);

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const onDragLeave = () => setIsDragging(false);

  const onDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);

      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) {
        const nonPdf = files.find(
          (file) => file.type !== 'application/pdf' && !file.name.toLowerCase().endsWith('.pdf'),
        );
        if (nonPdf) {
          toast({
            variant: 'destructive',
            title: 'Upload Error',
            description: 'Only PDF files are allowed',
          });
          return;
        }
      }

      // Fallback to file picker since we need absolute paths for Tauri
      handleFileUpload();
    },
    [handleFileUpload, toast],
  );

  // Pre-fill the issuer field with whatever partial hint the backend already
  // extracted (e.g. issuer resolved but masked account/card number missing).
  useEffect(() => {
    if (instrumentModalOpen) {
      setInstrumentIssuer(pendingInstrumentIssuerHint || '');
      setInstrumentMasked('');
      setInstrumentType('credit_card');
      setInstrumentError(null);
    }
  }, [instrumentModalOpen, pendingInstrumentIssuerHint]);

  // --- Statement Instrument Gate confirmation submit (C2) ---
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
      loadHistory();
    } catch (e: any) {
      setInstrumentError(e?.message || String(e) || 'Could not process the statement with these details.');
    } finally {
      setIsSubmittingInstrument(false);
    }
  }, [
    pendingInstrumentStatementId,
    instrumentIssuer,
    instrumentMasked,
    instrumentType,
    closeInstrumentModal,
    loadHistory,
    toast,
  ]);

  // --- Password submit ---
  const submitPassword = useCallback(async () => {
    if (!pendingStatementId || !password.trim()) return;
    setIsSubmittingPassword(true);
    setPasswordError(null);
    try {
      // I9 fix: the backend resolves (never throws) for both wrong-password
      // and max-attempts-exceeded outcomes — the `status` field, not promise
      // rejection, is what distinguishes them. Previously any non-throwing
      // response was treated as success, so wrong passwords silently showed
      // a false "Password Accepted" toast.
      const result = await API.statements.submitPassword(pendingStatementId, pendingInstrumentId, password);

      if (result.status === 'unlocked') {
        closePasswordModal();
        setPassword('');
        setPasswordError(null);
        toast({ title: 'Password Accepted', description: 'Retrying statement extraction…' });
        loadHistory();
      } else if (result.status === 'max_attempts_exceeded') {
        closePasswordModal();
        setPassword('');
        setPasswordError(null);
        toast({
          variant: 'destructive',
          title: 'Too Many Attempts',
          description: 'This statement is locked after 3 incorrect password attempts. Please re-upload it to try again.',
        });
        loadHistory();
      } else {
        // wrong_password — re-prompt without closing the modal
        const remaining = result.attempts_remaining;
        setPasswordError(
          remaining != null ? `Incorrect password — ${remaining} attempt${remaining === 1 ? '' : 's'} remaining` : 'Incorrect password',
        );
      }
    } catch (e) {
      setPasswordError('Incorrect password');
    } finally {
      setIsSubmittingPassword(false);
    }
  }, [pendingStatementId, pendingInstrumentId, password, closePasswordModal, loadHistory, toast]);

  // Derive unprocessed statements (PASSWORD_REQUIRED or FAILED)
  const unprocessedStatements = history.filter(
    (s) => s.status === 'PASSWORD_REQUIRED' || s.status === 'FAILED',
  );

  return (
    <div className="space-y-8 animate-in fade-in duration-500 h-[calc(100vh-80px)] flex flex-col">
      <header>
        <h1 className="text-3xl font-bold tracking-tight">Statements</h1>
        <p className="text-muted-foreground mt-1">Upload and manage your bank statements securely.</p>
      </header>

      {/* Upload Dropzone */}
      <Card
        className={cn(
          'border-2 border-dashed transition-colors cursor-pointer',
          isDragging
            ? 'border-primary bg-primary/10'
            : 'border-border hover:border-primary/50 hover:bg-secondary/50',
        )}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
        onClick={handleFileUpload}
        role="button"
        data-testid="dropzone"
        tabIndex={0}
        aria-label="Upload a PDF statement. Click or drag and drop."
        onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && handleFileUpload()}
      >
        <CardContent className="flex flex-col items-center justify-center py-12 text-center">
          <div className="w-16 h-16 rounded-full bg-secondary flex items-center justify-center mb-4" aria-hidden="true">
            <UploadCloud className="w-8 h-8 text-muted-foreground" />
          </div>
          <h2 className="text-lg font-semibold mb-1">Upload Statement</h2>
          <p className="text-sm text-muted-foreground mb-4">
            Drag and drop your PDF statements here, or click to browse.
          </p>
          <Button asChild variant="secondary" aria-hidden="true" tabIndex={-1}><span>Browse Files</span></Button>
        </CardContent>
      </Card>

      {/* Unprocessed Statements Queue */}
      {unprocessedStatements.length > 0 && (
        <Card className="border-amber-500/30 bg-amber-500/5">
          <CardHeader className="pb-3">
            <CardTitle className="flex items-center gap-2 text-amber-700">
              <AlertTriangle className="w-5 h-5" aria-hidden="true" />
              <span>Unprocessed Statements</span> <span>({unprocessedStatements.length})</span>
            </CardTitle>
            <CardDescription>These statements require action before they can be processed.</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-2"  aria-label="Unprocessed statements">
              {unprocessedStatements.map((stmt) => (
                <div
                  key={stmt.id}
                  className="flex items-center justify-between p-3 rounded-md bg-background border border-border"
                >
                  <div className="flex items-center gap-3">
                    <FileText className="w-4 h-4 text-muted-foreground shrink-0" aria-hidden="true" />
                    <div>
                      <p className="text-sm font-medium">{stmt.file_name}</p>
                      <p className="text-xs text-muted-foreground">{new Date(stmt.date).toLocaleDateString()}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant="destructive" className="text-xs">
                      {stmt.status === 'PASSWORD_REQUIRED' ? (
                        <><Lock className="w-3 h-3 mr-1" aria-hidden="true" />Password Required</>
                      ) : (
                        'Failed'
                      )}
                    </Badge>
                    {stmt.status === 'PASSWORD_REQUIRED' && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => openPasswordModal(stmt.id)}
                        aria-label={`Enter password for ${stmt.file_name}`}
                      >
                        <Lock className="w-3 h-3 mr-1" aria-hidden="true" />
                        Enter Password
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => stmt.status === 'PASSWORD_REQUIRED' ? openPasswordModal(stmt.id) : loadHistory()}
                      aria-label={`Retry processing ${stmt.file_name}`}
                    >
                      <RefreshCw className="w-3 h-3 mr-1" aria-hidden="true" />
                      Retry Processing
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

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
                            variant={
                              isProcessed
                                ? 'default'
                                : isLocked
                                ? 'destructive'
                                : isOCR
                                ? 'secondary'
                                : 'outline'
                            }
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
                              onClick={() => navigate(`/transactions?search=${encodeURIComponent(stmt.file_name)}`)}
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

      {/* Password Modal */}
      <Dialog open={passwordModalOpen} onOpenChange={(open) => {
          if (!open) {
            closePasswordModal();
            setPassword('');
            setPasswordError(null);
          }
        }}>
        <DialogContent
          className="sm:max-w-[425px]"
          aria-labelledby="password-dialog-title"
          aria-describedby="password-dialog-desc"
        >
          <DialogHeader>
            <DialogTitle id="password-dialog-title" className="flex items-center gap-2">
              <Lock className="w-5 h-5 text-red-700" aria-hidden="true" />
              Password Required
            </DialogTitle>
            <DialogDescription id="password-dialog-desc">
              The uploaded statement is encrypted. Please provide the PDF password to continue processing.
            </DialogDescription>
          </DialogHeader>

          {/* Countdown Timer */}
          <div
            className={cn(
              'flex items-center gap-2 text-sm px-3 py-2 rounded-md border',
              countdown <= 30
                ? 'text-red-700 bg-destructive/10 border-destructive/20'
                : 'text-muted-foreground bg-secondary border-border',
            )}
            role="timer"
            aria-live="polite"
            aria-label={`Time remaining to enter password: ${formatCountdown(countdown)}`}
          >
            <Clock className="w-4 h-4 shrink-0" aria-hidden="true" />
            <span>Time remaining: <strong>{formatCountdown(countdown)}</strong></span>
          </div>

          <div className="py-2 space-y-2">
            <Label htmlFor="pdf-password">PDF Password</Label>
            <Input
              id="pdf-password"
              type="password"
              placeholder="Enter PDF password"
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
                setPasswordError(null);
              }}
              onKeyDown={(e) => e.key === 'Enter' && submitPassword()}
              aria-invalid={!!passwordError}
              aria-describedby={passwordError ? 'password-error' : undefined}
              autoFocus
            />
            {passwordError && (
              <p id="password-error" role="alert" className="text-sm text-red-700 flex items-center gap-1">
                <AlertTriangle className="w-3 h-3" aria-hidden="true" />
                {passwordError}
              </p>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={closePasswordModal} aria-label="Cancel password entry">
              Cancel
            </Button>
            <Button
              onClick={submitPassword}
              disabled={!password.trim() || isSubmittingPassword}
              aria-label="Submit PDF password"
            >
              {isSubmittingPassword ? 'Unlocking…' : 'Unlock & Parse'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Statement Instrument Gate Confirmation Modal (C2) */}
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
        <DialogContent
          className="sm:max-w-[500px]"
          aria-labelledby="access-denied-title"
          aria-describedby="access-denied-desc"
        >
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
