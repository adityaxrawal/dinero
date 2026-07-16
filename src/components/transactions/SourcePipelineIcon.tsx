import { Mail, FileText, PenLine, HelpCircle } from 'lucide-react';

/**
 * G11 fix (extracted for TASK-FE-009/010 reuse): a row/detail-page icon
 * showing which ingestion pipeline produced a transaction.
 */
export default function SourcePipelineIcon({ sourceMix }: { sourceMix: string | null }) {
  const value = (sourceMix || '').toLowerCase();
  if (value.includes('statement')) {
    return <FileText className="w-3.5 h-3.5 text-muted-foreground" aria-label="From statement" />;
  }
  if (value.includes('manual')) {
    return <PenLine className="w-3.5 h-3.5 text-muted-foreground" aria-label="Manually entered" />;
  }
  if (value.includes('email') || value.includes('gmail')) {
    return <Mail className="w-3.5 h-3.5 text-muted-foreground" aria-label="From email" />;
  }
  return <HelpCircle className="w-3.5 h-3.5 text-muted-foreground" aria-label="Source unknown" />;
}
