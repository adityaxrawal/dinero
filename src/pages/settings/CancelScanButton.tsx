import { useState, useEffect } from 'react';
import { Loader2, XCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useGlobalState } from '@/lib/GlobalStateContext';
import CancelScanDialog from './CancelScanDialog';

export default function CancelScanButton() {
  const { scanStatus, handleCancelScan } = useGlobalState();

  const [isCancelling, setIsCancelling] = useState(false);
  useEffect(() => {
    if (scanStatus !== 'running') setIsCancelling(false);
  }, [scanStatus]);

  const [cancelDialogOpen, setCancelDialogOpen] = useState(false);
  const handleCancelClick = () => setCancelDialogOpen(true);
  const handleConfirmCancelScan = async () => {
    setCancelDialogOpen(false);
    setIsCancelling(true);
    try {
      await handleCancelScan();
    } catch {
      setIsCancelling(false);
    }
  };

  return (
    <>
      {scanStatus === 'running' && (
        <Button
          variant="outline"
          onClick={handleCancelClick}
          disabled={isCancelling}
          className="h-9 px-4 font-semibold border-red-300 text-red-600 hover:bg-red-50"
        >
          {isCancelling ? (
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
          ) : (
            <XCircle className="w-4 h-4 mr-2" />
          )}
          {isCancelling ? 'Cancelling…' : 'Cancel Scan'}
        </Button>
      )}

      <CancelScanDialog
        open={cancelDialogOpen}
        onOpenChange={setCancelDialogOpen}
        onConfirm={handleConfirmCancelScan}
      />
    </>
  );
}
