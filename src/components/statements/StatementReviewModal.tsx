/**
 * Review dialog for an extracted statement draft.
 *
 * The gate before anything reaches the ledger: rows and metadata are confirmed or
 * corrected here, so a misparsed statement is fixed rather than silently
 * absorbed.
 */
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { useStatementDraft } from './review/useStatementDraft';
import DraftMetadataForm from './review/DraftMetadataForm';
import DraftRowsEditor from './review/DraftRowsEditor';

const STAGE_LABELS: Record<string, string> = {
  parsing: 'Parsing PDF…',
  metadata: 'Reading statement details…',
  instrument_check: 'Identifying bank and card…',
  duplicate_check: 'Checking for duplicates…',
  extracting_rows: 'Extracting transactions…',
  staged: 'Ready for review',
};

/**
 * Review dialog for an extracted statement draft.
 *
 * The gate before anything reaches the ledger: rows and metadata are confirmed
 * or corrected here.
 */
export default function StatementReviewModal() {
  const draft = useStatementDraft();
  const { metadata, isStaged, processingProgress } = draft;

  return (
    <Dialog
      open={draft.reviewModalOpen}
      onOpenChange={(open) => {
        if (!open) draft.handleCancel();
      }}
    >
      <DialogContent
        className="sm:max-w-[1200px] w-[95vw] p-0 overflow-hidden flex flex-col max-h-[92vh] h-[820px]"
        aria-labelledby="review-dialog-title"
      >
        <div className="grid grid-cols-1 md:grid-cols-[minmax(0,1fr)_480px] flex-1 min-h-0 h-full">
          <div className="bg-[#F3EBDD] border-r flex flex-col min-h-0 min-w-0 h-full">
            {draft.pdfUrl ? (
              <iframe src={draft.pdfUrl} className="w-full h-full border-0" title="Statement PDF" />
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
                <DraftMetadataForm metadata={metadata} onChange={draft.setMetadata} />
                <DraftRowsEditor
                  rows={draft.rows}
                  onUpdateRow={draft.updateRow}
                  onAddRow={draft.addRow}
                  onRemoveRow={draft.removeRow}
                />
              </div>
            )}

            <DialogFooter className="pt-4 grid grid-cols-2 gap-3">
              <Button variant="outline" onClick={draft.handleCancel} disabled={draft.isDiscarding}>
                Cancel
              </Button>
              <Button onClick={draft.handleSubmit} disabled={!isStaged || draft.isSaving}>
                {draft.isSaving ? 'Saving…' : 'Submit'}
              </Button>
            </DialogFooter>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
