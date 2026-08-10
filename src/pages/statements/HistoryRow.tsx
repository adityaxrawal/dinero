/**
 * One statement in the processing history.
 */
import { AlertTriangle, ChevronRight, FileText, Trash2 } from 'lucide-react';
import { formatCustomDate } from '@/lib/formatCustomDate';
import type { useStatementsList } from '@/hooks/queries/useStatementsList';

type Statement = NonNullable<ReturnType<typeof useStatementsList>['data']>[number];

/** Readable name for a statement, falling back to its filename. */
function displayStatementName(stmt: Statement): string {
  if (stmt.issuer_name && stmt.masked_identifier) {
    const typeLabel = stmt.instrument_type === 'bank_account' ? 'Bank Account' : 'Credit Card';
    return `${stmt.issuer_name} ${typeLabel} •••${stmt.masked_identifier}`;
  }
  return stmt.file_name;
}

/** Pill showing the statement's processing status. */
function StatusPill({ status }: { status: string }) {
  const isProcessed = status === 'parsed';
  const isFailed = status === 'failed';
  return (
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
      {status.replace('_', ' ')}
    </span>
  );
}

/** One statement in the processing history. */
export default function HistoryRow({
  stmt,
  onViewPdf,
  onDeletePdf,
  onViewTransactions,
}: {
  stmt: Statement;
  onViewPdf: () => void;
  onDeletePdf: () => void;
  onViewTransactions: () => void;
}) {
  const isProcessed = stmt.status === 'parsed';
  const name = displayStatementName(stmt);
  const hasPdf = isProcessed && stmt.pdf_available;

  return (
    <div className="p-4 flex items-center justify-between transition-colors hover:bg-[#064E3B]/5">
      <div className="min-w-0 pr-4 flex-1">
        <div className="flex items-center gap-2 mb-1">
          <FileText className="w-4 h-4 flex-shrink-0 text-[#064E3B]/70" />
          <span className="text-[14px] font-semibold truncate text-[#064E3B]" title={name}>
            {name}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11px] font-medium text-[#064E3B]/60">
            {formatCustomDate(stmt.date)}
          </span>
          <StatusPill status={stmt.status} />
        </div>
      </div>

      <div className="flex items-center gap-2">
        {hasPdf && (
          <button
            type="button"
            className="h-8 px-3 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors bg-[#064E3B]/5 hover:bg-[#064E3B]/10 text-[#064E3B] text-[12px] font-medium gap-1.5"
            onClick={(e) => {
              e.stopPropagation();
              onViewPdf();
            }}
            aria-label="View statement PDF"
            title="View statement PDF"
          >
            <FileText className="w-3.5 h-3.5" />
            View PDF
          </button>
        )}
        {hasPdf && (
          <button
            type="button"
            className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 transition-colors bg-red-50 hover:bg-red-100 text-red-700"
            onClick={(e) => {
              e.stopPropagation();
              onDeletePdf();
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
            onClick={onViewTransactions}
            aria-label="View parsed transactions"
            title="View parsed transactions"
          >
            <ChevronRight className="w-4 h-4" />
          </button>
        )}
      </div>
    </div>
  );
}
