/**
 * Side-by-side comparison of the transactions in a cluster.
 *
 * The core of the merge decision: it highlights exactly which fields differ, so
 * the user can judge whether these are one payment or two.
 */
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
 * Colour for a match score.
 *
 * The threshold marks where the matcher would have been confident enough to
 * merge automatically, so the colour tells the user whether this cluster is a
 * near-certainty or a genuine judgement call.
 */
function scoreColor(score: number): string {
  return score >= 0.85 ? 'emerald' : 'amber';
}

/** Match-confidence bar with its percentage and an explanation of the blend. */
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

/**
 * One field, highlighted when it differs from the reference member.
 *
 * The highlight is the point of the whole comparison -- it directs attention to
 * exactly what disagrees rather than making the user scan every field.
 */
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

/**
 * The comparable fields of one cluster member.
 *
 * Instrument and reference render only when present, since a sparsely extracted
 * observation may have neither.
 */
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

/**
 * Side-by-side comparison of a cluster's members.
 *
 * The incoming observation is shown first as the reference, with candidates
 * below it diffed against it. That anchoring is deliberate: the question is
 * always "is this new evidence the same as one of these?", not an unanchored
 * comparison of everything against everything.
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
          {candidates.map((member) => (
            <CandidateCard
              key={member.id}
              member={member}
              reference={reference}
              isSelected={
                !!onSelectCandidate && member.canonical_transaction_id === selectedCandidateId
              }
              onSelect={onSelectCandidate}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** Renders the original email behind a member, as evidence for the decision. */
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
    // Malformed or absent payload JSON. The locals keep their empty defaults,
    // and the guard below renders nothing -- a source message that cannot be
    // parsed is simply not previewed, which is preferable to failing the whole
    // comparison view around it.
  }

  if (!html && !text) return null;

  return (
    <div className="mt-2">
      <GmailEmailViewer html={html} text={text} subject={subject} sender={sender} maxHeight="320px" />
    </div>
  );
}

/**
 * One candidate, selectable when the caller supports choosing a match.
 *
 * Given a button role and keyboard handlers when selectable, so the merge
 * decision is reachable without a mouse.
 */
function CandidateCard({
  member,
  reference,
  isSelected,
  onSelect,
}: {
  member: ClusterMember;
  reference: ClusterMember | undefined;
  isSelected: boolean;
  onSelect: ((member: ClusterMember) => void) | undefined;
}) {
  const selectable = !!onSelect;
  const diff = diffClusterMember(reference, member);

  return (
    <Card
      role={selectable ? 'button' : undefined}
      tabIndex={selectable ? 0 : undefined}
      onClick={selectable ? () => onSelect(member) : undefined}
      onKeyDown={
        selectable
          ? (e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onSelect(member);
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
}
