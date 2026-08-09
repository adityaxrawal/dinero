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

/** In-app confirm for cancelling a running scan. Deliberately a React Dialog
 *  rather than a native `ask()` — a native dialog renders outside React's tree
 *  and was found to overlap/garble the button in the Tauri webview. */
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
        className="sm:max-w-[440px] bg-[#F8E7C9] border-[#064E3B]/20 text-[#064E3B]"
        aria-labelledby="cancel-scan-dialog-title"
        aria-describedby="cancel-scan-dialog-desc"
      >
        <DialogHeader>
          <DialogTitle
            id="cancel-scan-dialog-title"
            className="flex items-center gap-2 text-[#064E3B]"
          >
            <XCircle className="w-5 h-5" aria-hidden="true" />
            Cancel Scan
          </DialogTitle>
          <DialogDescription
            id="cancel-scan-dialog-desc"
            className="text-[13px] pt-2 text-[#064E3B]/70"
          >
            Cancel the in-progress scan? Emails already processed keep their imported transactions.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
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
