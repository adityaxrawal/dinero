/**
 * One bank's learned rules, grouped into a single card.
 */
import { useState } from 'react';
import { ChevronRight, FileText, Loader2, Mail, Undo2 } from 'lucide-react';
import type { LearnedRule } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { InfoRow } from '@/components/ui/InfoRow';
import { cn } from '@/lib/utils';
import type { BankGroup, FormatGroup } from '../groupRules';
import { ConfidenceMeter, RelativeDate } from '../SettingsPrimitives';
import { FieldChip, StatusBadge } from './RuleBadges';
import { describeOrigin, describeReliability, describeRule, rulePattern, shortHash } from './labels';

/** Collapsible technical detail for one rule. */
function TechnicalDetail({ rule }: { rule: LearnedRule }) {
  const pattern = rulePattern(rule) || '—';
  return (
    <div className="mt-2 rounded-lg border border-[#064E3B]/10 bg-[#064E3B]/[0.02] overflow-hidden">
      <InfoRow label="What it does">{describeRule(rule)}</InfoRow>
      <InfoRow label="Email format" copyValue={rule.template_hash}>
        <span className="font-mono text-[12px]">{rule.template_hash || '—'}</span>
      </InfoRow>
      <InfoRow label="Pattern" copyValue={pattern}>
        <span className="font-mono text-[12px]">{pattern}</span>
      </InfoRow>
      {rule.rule_payload_json.capture_group !== undefined && (
        <InfoRow label="Capture group">{rule.rule_payload_json.capture_group}</InfoRow>
      )}
      <InfoRow label="Origin">{describeOrigin(rule)}</InfoRow>
      <InfoRow label="Track record">{describeReliability(rule)}</InfoRow>
      <InfoRow label="Confidence">{Math.round(rule.confidence * 100)}%</InfoRow>
      <InfoRow label="Last updated">
        <RelativeDate iso={rule.updated_at ?? rule.created_at} />
      </InfoRow>
    </div>
  );
}

/** Header for a template format group. */
function FormatHeader({ group, isLive }: { group: FormatGroup; isLive: boolean }) {
  const primary = group.rules[0];
  return (
    <div className="flex items-center gap-2 flex-wrap">
      {primary.source_type === 'email' ? (
        <Mail className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/45" />
      ) : (
        <FileText className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/45" />
      )}
      <span
        className="font-mono text-[12px] font-semibold text-[#064E3B] px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07]"
        title={`Email layout fingerprint ${primary.template_hash}`}
      >
        Format {shortHash(primary.template_hash)}
      </span>
      {group.fields.map((f) => (
        <FieldChip key={f} field={f} />
      ))}
      {!isLive && <StatusBadge status={primary.status} />}
      <ConfidenceMeter value={group.confidence} className="ml-auto" />
    </div>
  );
}

/** One rule row, with status, confidence and revert. */
function FormatRow({
  group,
  onRetire,
  revertingId,
}: {
  group: FormatGroup;
  onRetire: (rule: LearnedRule) => void;
  revertingId: string | null;
}) {
  const [open, setOpen] = useState(false);
  const primary = group.rules[0];
  const isLive = primary.status !== 'inactive' && primary.status !== 'flagged';
  const pattern = rulePattern(primary);

  return (
    <div className="px-4 py-3 border-b border-[#064E3B]/[0.07] last:border-0">
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <div className="min-w-0 flex-1">
          <FormatHeader group={group} isLive={isLive} />

          <p className="text-[12px] mt-1.5 text-[#064E3B]/60">
            learned <RelativeDate iso={group.learnedAt} /> · {describeReliability(primary)}
            {group.rules.length > 1 && ` · ${group.rules.length} rules`}
          </p>

          {pattern && (
            <p className="mt-1 font-mono text-[11px] text-[#064E3B]/45 truncate" title={pattern}>
              {pattern}
            </p>
          )}
        </div>

        {isLive && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => onRetire(primary)}
            disabled={revertingId === primary.id}
            className="shrink-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          >
            {revertingId === primary.id ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Undo2 className="w-3.5 h-3.5" />
            )}
            <span className="ml-1.5">Retire</span>
          </Button>
        )}
      </div>

      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="mt-2 text-[11px] text-[#064E3B]/50 hover:text-[#064E3B]/80 flex items-center gap-1"
      >
        <ChevronRight className={cn('w-3 h-3 transition-transform', open && 'rotate-90')} />
        Technical detail
      </button>
      {open && group.rules.map((r) => <TechnicalDetail key={r.id} rule={r} />)}
    </div>
  );
}

/** All of one bank's learned rules, grouped by template. */
export default function BankCard({
  group,
  defaultOpen,
  onRetire,
  revertingId,
}: {
  group: BankGroup;
  defaultOpen: boolean;
  onRetire: (rule: LearnedRule) => void;
  revertingId: string | null;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const fields = [...new Set(group.formats.flatMap((f) => f.fields))];

  return (
    <div className="rounded-xl border border-[#064E3B]/10 bg-white overflow-hidden">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full px-4 py-3 flex items-center gap-3 text-left hover:bg-[#064E3B]/[0.02] transition-colors"
      >
        <ChevronRight
          className={cn(
            'w-4 h-4 shrink-0 text-[#064E3B]/50 transition-transform',
            open && 'rotate-90'
          )}
        />
        <span className="font-bold text-[15px] text-[#064E3B] truncate">{group.bank}</span>
        <span className="text-[12px] text-[#064E3B]/55 shrink-0">
          {group.formats.length} format{group.formats.length === 1 ? '' : 's'} · {group.ruleCount}{' '}
          rule{group.ruleCount === 1 ? '' : 's'}
        </span>
        <span className="hidden sm:flex items-center gap-1 shrink-0">
          {fields.map((f) => (
            <FieldChip key={f} field={f} />
          ))}
        </span>
        <ConfidenceMeter value={group.confidence} className="ml-auto" />
      </button>

      {open && (
        <div className="border-t border-[#064E3B]/[0.07]">
          {group.formats.map((f) => (
            <FormatRow
              key={f.templateHash}
              group={f}
              onRetire={onRetire}
              revertingId={revertingId}
            />
          ))}
        </div>
      )}
    </div>
  );
}
