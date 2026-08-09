import { useNavigate } from 'react-router-dom';
import { FileText, Upload } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import type { useInstrumentForm } from '../useInstrumentForm';

type Statements = ReturnType<typeof useInstrumentForm>['instrumentStatements'];

function StatementCard({
  statement,
  onView,
}: {
  statement: Statements[number];
  onView: () => void;
}) {
  return (
    <div className="p-4 rounded-2xl bg-[#F8E7C9]/70 border border-[#064E3B]/10 space-y-2 shadow-xs">
      <div className="flex items-start justify-between gap-2">
        <span className="text-[13px] font-bold text-[#064E3B] truncate" title={statement.file_name}>
          {statement.file_name}
        </span>
        <span
          className={cn(
            'text-[10px] font-extrabold px-2.5 py-0.5 rounded-full uppercase tracking-wider shrink-0',
            statement.status === 'PROCESSED'
              ? 'bg-emerald-500/15 text-emerald-800 border border-emerald-500/20'
              : 'bg-amber-500/15 text-amber-900 border border-amber-500/20'
          )}
        >
          {statement.status}
        </span>
      </div>
      <div className="flex items-center justify-between text-[11px] text-[#064E3B]/70 font-medium pt-1 border-t border-[#064E3B]/10">
        <span>Uploaded {formatCustomDate(statement.date)}</span>
        <button
          type="button"
          onClick={onView}
          className="font-bold text-[#064E3B] hover:underline"
        >
          View details →
        </button>
      </div>
    </div>
  );
}

export default function StatementsTab({ statements }: { statements: Statements }) {
  const navigate = useNavigate();
  const goToStatements = () => navigate('/statements');

  return (
    <div className="space-y-4 animate-in fade-in-50 duration-200">
      <div className="flex items-center justify-between">
        <span className="text-xs font-bold uppercase tracking-wider text-[#064E3B]/70 flex items-center gap-1.5">
          <FileText className="w-3.5 h-3.5" /> Uploaded Statements ({statements.length})
        </span>
        <button
          type="button"
          onClick={goToStatements}
          className="text-xs font-bold px-3 py-1.5 rounded-xl bg-[#064E3B] text-[#F8E7C9] flex items-center gap-1.5 hover:bg-[#064E3B]/90 cursor-pointer shadow-xs"
        >
          <Upload className="w-3.5 h-3.5" /> Upload Statement
        </button>
      </div>

      {statements.length === 0 ? (
        <div className="text-center py-12 bg-[#F8E7C9]/40 rounded-2xl border border-[#064E3B]/10 space-y-3">
          <FileText className="w-8 h-8 text-[#064E3B]/40 mx-auto" />
          <p className="text-xs text-[#064E3B]/60 italic">
            No statements uploaded yet for this account.
          </p>
          <button
            type="button"
            onClick={goToStatements}
            className="text-xs font-bold text-[#064E3B] underline cursor-pointer"
          >
            Go to Statements Center to import PDFs
          </button>
        </div>
      ) : (
        <div className="space-y-2.5">
          {statements.map((s) => (
            <StatementCard key={s.id} statement={s} onView={goToStatements} />
          ))}
        </div>
      )}
    </div>
  );
}
