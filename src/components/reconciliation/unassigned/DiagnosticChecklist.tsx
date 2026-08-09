import { CheckCircle2, XCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { DiagnosticCheck } from './diagnostics';

function CheckRow({ check }: { check: DiagnosticCheck }) {
  return (
    <div
      className={cn(
        'flex items-start gap-2 p-2 rounded-md border',
        check.passed
          ? 'bg-emerald-50/60 border-emerald-200/60 text-emerald-900'
          : 'bg-red-50/60 border-red-200/60 text-red-900'
      )}
    >
      {check.passed ? (
        <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 flex-shrink-0 mt-0.5" />
      ) : (
        <XCircle className="w-3.5 h-3.5 text-red-600 flex-shrink-0 mt-0.5" />
      )}
      <div className="min-w-0 flex-1">
        <div className="font-semibold text-[11px] flex items-center justify-between gap-1">
          <span>{check.label}</span>
          <span
            className={cn(
              'text-[9px] font-extrabold uppercase px-1 rounded',
              check.passed ? 'bg-emerald-200/60 text-emerald-900' : 'bg-red-200/60 text-red-900'
            )}
          >
            {check.passed ? 'Passed' : 'Failed'}
          </span>
        </div>
        <p className="text-[11px] opacity-80 truncate">{check.value}</p>
      </div>
    </div>
  );
}

export default function DiagnosticChecklist({ checks }: { checks: DiagnosticCheck[] }) {
  return (
    <div className="bg-white/90 rounded-lg p-3 border border-[#064E3B]/10">
      <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 mb-2.5">
        Ingestion Diagnostic Checklist
      </h4>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-2 text-xs">
        {checks.map((check) => (
          <CheckRow key={check.id} check={check} />
        ))}
      </div>
    </div>
  );
}
