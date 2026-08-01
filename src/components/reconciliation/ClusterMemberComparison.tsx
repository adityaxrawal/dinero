import type { ReactNode } from 'react';
import { Check } from 'lucide-react';
import { Link } from 'react-router-dom';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import SourcePipelineIcon from '@/components/transactions/SourcePipelineIcon';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { cn } from '@/lib/utils';
import type { ClusterMember } from '@/lib/ipc';
import { GmailEmailViewer } from '@/components/common/GmailEmailViewer';
import { diffClusterMember } from '@/lib/clusterDiff';

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
 * Layout: the "incoming" member is pinned full-width at the top as the
 * fixed reference every candidate is diffed against, with candidates
 * stacked full-width below -- never a horizontal scroll strip, which used
 * to crop the reference card itself off-screen.
 *
 * `onSelectCandidate` is optional: only the cluster detail page's
 * "Confirm Match" flow needs the user to pick which candidate is the real
 * match; the review queue's compact preview renders read-only.
 */
/** Score-zone color: amber for the ambiguous mid-range, emerald once a candidate is clearly ahead. */
function scoreColor(score: number): string {
  return score >= 0.85 ? 'emerald' : 'amber';
}

function ScoreBar({ score }: { score: number }) {
  const color = scoreColor(score);
  return (
    <div>
      <div className="flex items-center gap-1.5 w-24">
        <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
          <div
            className={cn('h-full rounded-full', color === 'emerald' ? 'bg-emerald-500' : 'bg-amber-500')}
            style={{ width: `${Math.round(score * 100)}%` }}
          />
        </div>
        <span className="text-xs font-medium text-muted-foreground shrink-0">
          {Math.round(score * 100)}%
        </span>
      </div>
      <p className="text-[10px] text-muted-foreground mt-0.5">Blends merchant, amount &amp; date match</p>
    </div>
  );
}

/** Field cell that highlights itself when its value differs from the reference ("New Evidence") member. */
function DiffField({
  label,
  value,
  diff,
  className,
}: {
  label: string;
  value: ReactNode;
  diff: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'rounded-md',
        diff && 'border-l-2 border-amber-400 bg-amber-50/60 -mx-1 px-2 py-1'
      )}
    >
      <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">{label}</p>
      <p className={className}>{value}</p>
    </div>
  );
}

function MemberFields({
  member,
  diff,
}: {
  member: ClusterMember;
  diff: { merchant: boolean; instrument: boolean; amount: boolean; date: boolean };
}) {
  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
      <DiffField label="Merchant" value={member.merchant} diff={diff.merchant} className="font-medium" />
      {(member.instrument_issuer_name || member.instrument_masked_identifier) && (
        <DiffField
          label="Instrument"
          diff={diff.instrument}
          className="font-medium"
          value={
            <>
              {member.instrument_issuer_name}
              {member.instrument_masked_identifier && ` •• ${member.instrument_masked_identifier}`}
            </>
          }
        />
      )}
      <DiffField
        label="Amount"
        diff={diff.amount}
        className={cn('font-medium', member.direction === 'debit' ? 'text-red-700' : 'text-emerald-700')}
        value={`${member.direction === 'debit' ? '-' : '+'} ₹${Math.abs(member.amount).toFixed(2)}`}
      />
      <DiffField
        label="Date"
        diff={diff.date}
        className="text-sm"
        value={member.date === 'Unknown' ? 'Unknown' : formatCustomDate(member.date)}
      />
      {member.reference_id && (
        <div>
          <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">Reference ID</p>
          <p className="text-sm font-mono">{member.reference_id}</p>
        </div>
      )}
    </div>
  );
}

export default function ClusterMemberComparison({
  members,
  selectedCandidateId,
  onSelectCandidate,
}: {
  members: ClusterMember[];
  selectedCandidateId?: string | null;
  onSelectCandidate?: (member: ClusterMember) => void;
}) {
  const reference = members.find((m) => m.member_role === 'incoming');
  const candidates = members.filter((m) => m !== reference);

  return (
    <div className="flex flex-col gap-4">
      {reference && (
        <Card className="border-[#064E3B]/30 shadow-sm">
          <CardHeader className="py-3 px-4 bg-[#064E3B]/5 border-b border-border/40">
            <CardTitle className="text-sm flex items-center justify-between gap-2">
              <span className="flex items-center gap-2">
                <span className="text-[9px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider bg-[#064E3B]/10 text-[#064E3B]">
                  New Evidence
                </span>
              </span>
              <SourcePipelineIcon sourceMix={reference.source_pipeline} />
            </CardTitle>
          </CardHeader>
          <CardContent className="p-4">
            <MemberFields
              member={reference}
              diff={{ merchant: false, instrument: false, amount: false, date: false }}
            />
          </CardContent>
          {reference.source_raw_payload_json && (
            <details className="px-4 pb-3">
              <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                View source message
              </summary>
              <SourceMessagePreview rawPayloadJson={reference.source_raw_payload_json} />
            </details>
          )}
        </Card>
      )}

      {candidates.length > 0 && (
        <div className="flex flex-col gap-3">
          {candidates.map((member) => {
            const selectable = !!onSelectCandidate;
            const isSelected = selectable && member.canonical_transaction_id === selectedCandidateId;
            const diff = diffClusterMember(reference, member);

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
                  'w-full',
                  selectable && 'cursor-pointer hover:border-primary/60',
                  isSelected && 'border-emerald-500 ring-2 ring-emerald-500/30'
                )}
              >
                <CardHeader className="py-3 px-4 bg-muted/30 border-b border-border/40">
                  <CardTitle className="text-sm flex items-center justify-between gap-2">
                    <span className="flex items-center gap-2">
                      {ROLE_LABEL[member.member_role] ?? member.member_role}
                      {isSelected && (
                        <Check className="w-3.5 h-3.5 text-emerald-600" aria-label="Selected as match" />
                      )}
                    </span>
                    <span className="flex items-center gap-3 text-muted-foreground font-normal">
                      {member.match_score !== null && <ScoreBar score={member.match_score} />}
                      <SourcePipelineIcon sourceMix={member.source_pipeline} />
                    </span>
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-4 space-y-3">
                  <MemberFields member={member} diff={diff} />
                  {member.canonical_transaction_id && (
                    <div className="pt-1">
                      <Link
                        to={`/transactions/${member.canonical_transaction_id}`}
                        onClick={(e) => e.stopPropagation()}
                        className="text-xs text-primary hover:underline"
                      >
                        View full transaction →
                      </Link>
                    </div>
                  )}
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

function SourceMessagePreview({ rawPayloadJson }: { rawPayloadJson: string }) {
  let html = '';
  let text = '';
  let subject = '';
  let sender = '';
  try {
    const parsed = JSON.parse(rawPayloadJson);
    html = parsed.html || '';
    text = parsed.body || parsed.snippet || '';
    subject = parsed.subject || '';
    sender = parsed.sender || parsed.from || '';
  } catch {
    // malformed payload -- leave both empty, nothing to show
  }

  if (!html && !text) return null;

  return (
    <div className="mt-2">
      <GmailEmailViewer html={html} text={text} subject={subject} sender={sender} maxHeight="320px" />
    </div>
  );
}
