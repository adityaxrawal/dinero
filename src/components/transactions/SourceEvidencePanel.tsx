import { ShieldCheck } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { TransactionObservation } from '@/lib/ipc';
import SourcePipelineIcon from './SourcePipelineIcon';
import { evidenceDescription } from './evidenceDescription';

interface SourceEvidencePanelProps {
  observations: TransactionObservation[];
}

/**
 * TASK-FE-010 (Doc 30): shows every linked observation, which pipeline
 * produced it, extraction method/confidence, and — for statement-sourced
 * observations — an explicit "confirmed by your bank statement" note,
 * satisfying the statement-overrides-email transparency requirement
 * (Document 15 Core Principle: statement precedence over email).
 */
export default function SourceEvidencePanel({ observations }: SourceEvidencePanelProps) {
  if (observations.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Source Evidence</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">No linked observations found for this transaction.</p>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Source Evidence</CardTitle>
        <CardDescription>Every record that contributed to this transaction.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {observations.map((obs) => {
          const isStatementSourced = obs.source_pipeline === 'statement_pdf';
          const { label, detail } = evidenceDescription(obs.source_pipeline);
          return (
            <div key={obs.id} className="p-3 bg-secondary/50 rounded-md border-l-2 border-primary text-sm space-y-1.5">
              <div className="flex items-center justify-between">
                <p className="font-medium flex items-center gap-2">
                  <SourcePipelineIcon sourceMix={obs.source_pipeline} />
                  {label}
                </p>
                {obs.confidence_score !== null && (
                  <Badge variant="outline" className="text-[10px]">
                    {Math.round(obs.confidence_score * 100)}% confidence
                  </Badge>
                )}
              </div>
              <p className="text-xs text-muted-foreground">{detail}</p>
              {obs.extraction_method && (
                <p className="text-xs text-muted-foreground">
                  Extraction method: <span className="font-mono">{obs.extraction_method}</span>
                </p>
              )}
              {isStatementSourced && (
                <p className="flex items-center gap-1.5 text-xs text-emerald-700 font-medium pt-1">
                  <ShieldCheck className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
                  This value was confirmed by your bank statement.
                </p>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
