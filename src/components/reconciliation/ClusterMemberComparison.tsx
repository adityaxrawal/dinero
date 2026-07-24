import { Check } from 'lucide-react';
import { Link } from 'react-router-dom';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import SourcePipelineIcon from '@/components/transactions/SourcePipelineIcon';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { cn } from '@/lib/utils';
import type { ClusterMember } from '@/lib/ipc';

const ROLE_LABEL: Record<string, string> = {
  incoming: 'New Evidence',
  candidate_a: 'Existing Match A',
  candidate_b: 'Existing Match B',
  candidate_other: 'Existing Match',
};

/**
 * TASK-FE-013 (Doc 30): side-by-side member comparison "designed to make
 * ambiguity visually obvious" -- merchant, amount, date, source pipeline.
 * No confidence score column: Document 18 §4.6/§4.6a has no such field on
 * either `reconciliation_clusters` or `reconciliation_cluster_members`, so
 * one is never fabricated here (the pre-rewrite page hardcoded a static
 * "Confidence: 75%" badge on every cluster regardless of its real data).
 *
 * `onSelectCandidate` is optional: only the cluster detail page's
 * "Confirm Match" flow needs the user to pick which candidate is the real
 * match; the review queue's compact preview renders read-only.
 */
export default function ClusterMemberComparison({
  members,
  selectedCandidateId,
  onSelectCandidate,
}: {
  members: ClusterMember[];
  selectedCandidateId?: string | null;
  onSelectCandidate?: (member: ClusterMember) => void;
}) {
  return (
    <div className="flex gap-4 overflow-x-auto pb-2 snap-x">
      {members.map((member) => {
        const isCandidate = member.member_role !== 'incoming';
        const selectable = isCandidate && !!onSelectCandidate;
        const isSelected = selectable && member.canonical_transaction_id === selectedCandidateId;

        return (
          <Card
            key={member.id}
            role={selectable ? 'button' : undefined}
            tabIndex={selectable ? 0 : undefined}
            onClick={selectable ? () => onSelectCandidate!(member) : undefined}
            onKeyDown={
              selectable
                ? (e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      onSelectCandidate!(member);
                    }
                  }
                : undefined
            }
            className={cn(
              'min-w-[240px] shrink-0 snap-center',
              selectable && 'cursor-pointer hover:border-primary/60',
              isSelected && 'border-emerald-500 ring-2 ring-emerald-500/30'
            )}
          >
            <CardHeader className="py-3 px-4 bg-muted/30 border-b border-border/40">
              <CardTitle className="text-sm flex items-center justify-between gap-2">
                <span>{ROLE_LABEL[member.member_role] ?? member.member_role}</span>
                <span className="flex items-center gap-1 text-muted-foreground font-normal">
                  <SourcePipelineIcon sourceMix={member.source_pipeline} />
                  {isCandidate && member.match_score !== null && (
                    <span className="text-xs font-medium text-muted-foreground">
                      {Math.round(member.match_score * 100)}%
                    </span>
                  )}
                  {isSelected && (
                    <Check
                      className="w-3.5 h-3.5 text-emerald-600"
                      aria-label="Selected as match"
                    />
                  )}
                </span>
              </CardTitle>
            </CardHeader>
            <CardContent className="p-4 space-y-3">
              <div>
                <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">
                  Merchant
                </p>
                <p className="font-medium">{member.merchant}</p>
              </div>
              {(member.instrument_issuer_name || member.instrument_masked_identifier) && (
                <div>
                  <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">
                    Instrument
                  </p>
                  <p className="font-medium">
                    {member.instrument_issuer_name}
                    {member.instrument_masked_identifier &&
                      ` •• ${member.instrument_masked_identifier}`}
                  </p>
                </div>
              )}
              <div>
                <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">
                  Amount
                </p>
                <p
                  className={cn(
                    'font-medium',
                    member.direction === 'debit' ? 'text-red-700' : 'text-emerald-700'
                  )}
                >
                  {member.direction === 'debit' ? '-' : '+'} ₹{Math.abs(member.amount).toFixed(2)}
                </p>
              </div>
              <div>
                <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">Date</p>
                <p className="text-sm">
                  {member.date === 'Unknown' ? 'Unknown' : formatCustomDate(member.date)}
                </p>
              </div>
              {member.reference_id && (
                <div>
                  <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">
                    Reference ID
                  </p>
                  <p className="text-sm font-mono">{member.reference_id}</p>
                </div>
              )}
            </CardContent>
            {!isCandidate && member.source_raw_payload_json && (
              <details className="px-4 pb-3">
                <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                  View source message
                </summary>
                <SourceMessagePreview rawPayloadJson={member.source_raw_payload_json} />
              </details>
            )}
            {isCandidate && member.canonical_transaction_id && (
              <div className="px-4 pb-3">
                <Link
                  to={`/transactions/${member.canonical_transaction_id}`}
                  className="text-xs text-primary hover:underline"
                >
                  View full transaction →
                </Link>
              </div>
            )}
          </Card>
        );
      })}
    </div>
  );
}

function SourceMessagePreview({ rawPayloadJson }: { rawPayloadJson: string }) {
  let html = '';
  let text = '';
  try {
    const parsed = JSON.parse(rawPayloadJson);
    html = parsed.html || '';
    text = parsed.body || parsed.snippet || '';
  } catch {
    // malformed payload -- leave both empty, nothing to show
  }
  if (html) {
    return (
      <iframe
        srcDoc={html}
        className="w-full h-[300px] rounded-md border border-border mt-2 bg-white"
        sandbox="allow-same-origin"
        title="Source message"
      />
    );
  }
  return (
    <p className="text-xs text-muted-foreground mt-2 whitespace-pre-wrap font-mono">{text}</p>
  );
}
