import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { ScrollArea } from '@/components/ui/scroll-area';
import { JsonViewer } from '@/components/ui/JsonViewer';

function SourceBody({ isLoading, data }: { isLoading: boolean; data: unknown }) {
  if (isLoading) return <>Loading...</>;
  if (!data) return <>No data</>;
  return (
    <div className="py-2">
      {typeof data === 'string' ? (
        <pre className="whitespace-pre-wrap">{data}</pre>
      ) : (
        <JsonViewer data={data} />
      )}
    </div>
  );
}

export default function RawSourceDialog({
  open,
  onOpenChange,
  isLoading,
  data,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  isLoading: boolean;
  data: unknown;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Transaction Source Data</DialogTitle>
          <DialogDescription>
            Exact email/statement data parsed for this transaction.
          </DialogDescription>
        </DialogHeader>
        <ScrollArea className="flex-1 mt-4 p-4 bg-black/40 rounded-md font-mono text-sm">
          <SourceBody isLoading={isLoading} data={data} />
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}
