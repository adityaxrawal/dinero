/**
 * Displays a stored statement PDF.
 */
import { useEffect, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';

/** Converts base64 PDF bytes into a blob for display. */
function base64ToBlob(base64: string): Blob {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new Blob([bytes], { type: 'application/pdf' });
}

/** Displays a stored statement PDF. */
export default function StatementPdfViewerModal({
  statementId,
  open,
  onOpenChange,
}: {
  statementId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { toast } = useToast();
  const [objectUrl, setObjectUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !statementId) {
      setObjectUrl(null);
      return;
    }
    let cancelled = false;
    API.statements
      .getPdf(statementId)
      .then((base64) => {
        if (cancelled) return;
        const blob = base64ToBlob(base64);
        setObjectUrl(URL.createObjectURL(blob));
      })
      .catch(() => {
        if (cancelled) return;
        toast({
          title: 'PDF unavailable',
          description: "This statement's PDF is no longer available.",
          variant: 'destructive',
        });
        onOpenChange(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, statementId]);

  useEffect(() => {
    return () => {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [objectUrl]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[900px] w-[90vw] h-[85vh] p-0 overflow-hidden flex flex-col">
        <DialogHeader className="px-6 pt-6 pb-2">
          <DialogTitle>Statement PDF</DialogTitle>
        </DialogHeader>
        <div className="flex-1 min-h-0 px-6 pb-6">
          {objectUrl ? (
            <iframe
              src={objectUrl}
              className="w-full h-full border-0 rounded-lg"
              title="Statement PDF"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-sm text-muted-foreground">
              Loading…
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
