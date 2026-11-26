/**
 * Confirmation for cancelling a scan, warning that progress is kept but stopped.
 */
import { XCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/** Confirms cancelling a scan, noting that progress is kept. */
export default function CancelScanDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-[440px] bg-champagne border-emerald-ink/20 text-emerald-ink"
        aria-labelledby="cancel-scan-dialog-title"
        aria-describedby="cancel-scan-dialog-desc"
      >
        <DialogHeader>
          <DialogTitle
            id="cancel-scan-dialog-title"
            className="flex items-center gap-2 text-emerald-ink"
          >
            <XCircle className="w-5 h-5" aria-hidden="true" />
            Cancel Scan
          </DialogTitle>
          <DialogDescription
            id="cancel-scan-dialog-desc"
            className="text-[13px] pt-2 text-emerald-ink/70"
          >
            Cancel the in-progress scan? Emails already processed keep their imported transactions.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            className="border-emerald-ink/20 text-emerald-ink hover:bg-emerald-ink/5"
            onClick={() => onOpenChange(false)}
          >
            Keep Scanning
          </Button>
          <Button className="bg-red-600 text-white hover:bg-red-700" onClick={onConfirm}>
            Cancel Scan
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
