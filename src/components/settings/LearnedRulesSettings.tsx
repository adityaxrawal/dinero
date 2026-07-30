import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  GraduationCap,
  Loader2,
  Undo2,
  AlertTriangle,
  ChevronRight,
  Mail,
  FileText,
  Landmark,
  ShieldCheck,
  Layers,
  AtSign,
  Sparkles,
} from 'lucide-react';
import { API, type LearnedRule, type SenderBankOverride } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { InfoRow } from '@/components/ui/InfoRow';
import { toast } from '@/hooks/use-toast';
import { cn } from '@/lib/utils';
import {
  ConfidenceMeter,
  ConfirmDialog,
  RelativeDate,
  StatStrip,
  StatTile,
} from './SettingsPrimitives';

/**
 * Read-only visibility into what the extraction pipeline has taught itself.
 *
 * This replaces the old "approve this regex" queue, and the read-only part is
 * the point rather than a simplification: a rule is written only after it has
 * mechanically proved it reproduces a real correction and does not change any
 * answer the bank's history already settled on. Asking a person to re-check
 * that by reading `([\d,]+(?:\.\d+)?)\s*(?:INR|Rs)` was never a reasonable
 * request, and it is not one this panel makes.
 *
 * The organising principle is the *email format*, not the rule. Every rule for
 * one bank and field describes itself with the same sentence ("Reads the
 * merchant name out of Yes Bank email alerts"), so a flat list of them reads as
 * a duplicate-rows bug. What actually differs is `template_hash` — which layout
 * of that bank's mail the rule applies to — so that is what the rows are keyed
 * on. The regex sits behind a disclosure for the rare case someone is
 * debugging. The one judgment left to a human is "this rule is misbehaving,
 * retire it".
 */

const FIELD_LABELS: Record<string, string> = {
  merchant: 'the merchant name',
  amount: 'the amount',
  event_time: 'the transaction date',
  reference_id: 'the reference number',
  balance: 'the closing balance',
  last4: 'the card/account digits',
  direction: 'whether it is money in or out',
  currency: 'the currency',
};

/** Short chip form of a field name. */
const FIELD_CHIPS: Record<string, string> = {
  merchant: 'merchant',
  amount: 'amount',
  event_time: 'date',
  reference_id: 'reference',
  balance: 'balance',
  last4: 'card digits',
  direction: 'in/out',
  currency: 'currency',
};

/** Human-readable description of what a rule actually does. */
function describeRule(rule: LearnedRule): string {
  const source = rule.source_type === 'email' ? 'email alerts' : 'PDF statements';
  const field = FIELD_LABELS[rule.field_name] ?? rule.field_name;

  if (rule.rule_payload_json.override_value !== undefined) {
    return `Always reads ${field} as "${rule.rule_payload_json.override_value}" for one ${rule.bank_name} ${source} format.`;
  }
  return `Reads ${field} out of ${rule.bank_name} ${source}.`;
}

/** Where the rule came from, in words. */
function describeOrigin(rule: LearnedRule): string {
  const trigger =
    {
      user_edit: 'Learned from a correction you made',
      drift_llm: "Learned automatically after this bank's format changed",
      batch_cleanup: 'Learned during a merchant cleanup run',
    }[rule.learned_from] ?? rule.learned_from;
  const author =
    rule.authored_by === 'llm' ? 'written by the on-device AI' : 'derived directly from the text';
  return `${trigger}, ${author}.`;
}

/** Track record in words rather than two bare counters. */
function describeReliability(rule: LearnedRule): string {
  const worked = `worked ${rule.success_count} time${rule.success_count === 1 ? '' : 's'}`;
  return rule.failure_count > 0
    ? `${worked} · corrected again ${rule.failure_count}×`
    : `${worked} · never wrong`;
}

const STATUS_STYLES: Record<string, string> = {
  trusted: 'bg-emerald-100 text-emerald-900 border-emerald-300',
  active: 'bg-[#F8E7C9] text-[#064E3B] border-[#064E3B]/20',
  pending: 'bg-amber-50 text-amber-900 border-amber-300',
  inactive: 'bg-neutral-100 text-neutral-600 border-neutral-300',
  flagged: 'bg-red-50 text-red-900 border-red-300',
};

const STATUS_LABELS: Record<string, string> = {
  trusted: 'Proven',
  active: 'In use',
  pending: 'On trial',
  inactive: 'Retired',
  flagged: 'Flagged',
};

function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={cn(
        'text-[10px] font-bold uppercase tracking-wide px-2 py-0.5 rounded-full border shrink-0',
        STATUS_STYLES[status] ?? STATUS_STYLES.inactive
      )}
    >
      {STATUS_LABELS[status] ?? status}
    </span>
  );
}

function FieldChip({ field }: { field: string }) {
  return (
    <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07] text-[#064E3B]/75">
      {FIELD_CHIPS[field] ?? field}
    </span>
  );
}

/** First 4 hex characters — enough to tell two formats apart at a glance. */
function shortHash(hash: string): string {
  return (hash || '????').slice(0, 4).toUpperCase();
}

type FormatGroup = {
  templateHash: string;
  rules: LearnedRule[];
  /** Lowest confidence in the group — the one worth acting on. */
  confidence: number;
  fields: string[];
  learnedAt: string | null;
};

type BankGroup = {
  bank: string;
  formats: FormatGroup[];
  ruleCount: number;
  confidence: number;
};

/**
 * Bank → email format → the rules that format teaches.
 *
 * Two rules sharing a bank and field but not a `template_hash` are genuinely
 * different rules about different mail, and must never collapse into one row.
 */
export function groupRules(rules: LearnedRule[]): BankGroup[] {
  const banks = new Map<string, Map<string, LearnedRule[]>>();
  for (const rule of rules) {
    const formats = banks.get(rule.bank_name) ?? new Map<string, LearnedRule[]>();
    banks.set(rule.bank_name, formats);
    const bucket = formats.get(rule.template_hash) ?? [];
    bucket.push(rule);
    formats.set(rule.template_hash, bucket);
  }

  return [...banks.entries()]
    .map(([bank, formats]) => {
      const groups: FormatGroup[] = [...formats.entries()]
        .map(([templateHash, groupRules]) => ({
          templateHash,
          rules: groupRules,
          confidence: Math.min(...groupRules.map((r) => r.confidence)),
          fields: [...new Set(groupRules.map((r) => r.field_name))],
          learnedAt:
            groupRules
              .map((r) => r.created_at)
              .filter((d): d is string => !!d)
              .sort()
              .at(-1) ?? null,
        }))
        // Newest format first: the thing that just changed is the thing worth
        // looking at.
        .sort((a, b) => (b.learnedAt ?? '').localeCompare(a.learnedAt ?? ''));

      const all = groups.flatMap((g) => g.rules);
      return {
        bank,
        formats: groups,
        ruleCount: all.length,
        confidence: all.reduce((sum, r) => sum + r.confidence, 0) / all.length,
      };
    })
    .sort((a, b) => b.ruleCount - a.ruleCount || a.bank.localeCompare(b.bank));
}

function TechnicalDetail({ rule }: { rule: LearnedRule }) {
  const pattern = rule.rule_payload_json.regex ?? rule.rule_payload_json.override_value ?? '—';
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
  const pattern = primary.rule_payload_json.regex ?? primary.rule_payload_json.override_value ?? '';

  return (
    <div className="px-4 py-3 border-b border-[#064E3B]/[0.07] last:border-0">
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <div className="min-w-0 flex-1">
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

function BankCard({
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

const FIELD_FILTERS = ['all', 'merchant', 'amount', 'event_time'] as const;
type FieldFilter = (typeof FIELD_FILTERS)[number];
type SortMode = 'default' | 'weakest';

export default function LearnedRulesSettings() {
  const [rules, setRules] = useState<LearnedRule[] | null>(null);
  const [overrides, setOverrides] = useState<SenderBankOverride[] | null>(null);
  const [revertingId, setRevertingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showRetired, setShowRetired] = useState(false);
  const [fieldFilter, setFieldFilter] = useState<FieldFilter>('all');
  const [sortMode, setSortMode] = useState<SortMode>('default');
  const [pendingRule, setPendingRule] = useState<LearnedRule | null>(null);
  const [pendingOverride, setPendingOverride] = useState<SenderBankOverride | null>(null);

  const load = useCallback(async () => {
    try {
      const [r, o] = await Promise.all([API.learnedRules.list(), API.senderOverrides.list()]);
      setRules(r);
      setOverrides(o);
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const revertRule = async (rule: LearnedRule) => {
    setRevertingId(rule.id);
    setError(null);
    try {
      await API.learnedRules.revert(rule.id);
      await load();
      toast({
        title: 'Rule retired',
        description: `Future scans read ${FIELD_LABELS[rule.field_name] ?? rule.field_name} from ${rule.bank_name} the way they did before this was learned. Nothing already recorded changed.`,
      });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setRevertingId(null);
    }
  };

  const revertOverride = async (o: SenderBankOverride) => {
    setRevertingId(o.id);
    setError(null);
    try {
      await API.senderOverrides.revert(o.id);
      await load();
      toast({
        title: 'Correction removed',
        description: `Mail from ${o.domain} goes back to whichever bank the built-in list names.`,
      });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setRevertingId(null);
    }
  };

  const { live, retired } = useMemo(() => {
    const all = rules ?? [];
    return {
      live: all.filter((r) => r.status !== 'inactive' && r.status !== 'flagged'),
      retired: all.filter((r) => r.status === 'inactive' || r.status === 'flagged'),
    };
  }, [rules]);

  const banks = useMemo(() => {
    const filtered =
      fieldFilter === 'all' ? live : live.filter((r) => r.field_name === fieldFilter);
    const grouped = groupRules(filtered);
    return sortMode === 'weakest'
      ? [...grouped].sort((a, b) => a.confidence - b.confidence)
      : grouped;
  }, [live, fieldFilter, sortMode]);

  const totals = useMemo(
    () => ({
      banks: new Set(live.map((r) => r.bank_name)).size,
      corrections: live.reduce((sum, r) => sum + r.success_count, 0),
    }),
    [live]
  );

  /** Only worth showing controls once the list is long enough to need them. */
  const showControls = live.length > 6;
  const activeOverrides = overrides?.filter((o) => o.status === 'active') ?? [];

  return (
    <section>
      <div className="mb-5">
        <h2 className="text-xl font-bold flex items-center gap-2">
          <GraduationCap className="w-5 h-5" /> What Dinero Has Learned
        </h2>
        <p className="text-sm mt-1 text-[#064E3B]/70 leading-relaxed">
          Every time you correct a merchant, amount or date, Dinero works out the rule behind the
          mistake and checks it against your own history before using it — so the same bank&apos;s
          next email gets read correctly without you doing anything. Rules are grouped by the email
          layout they apply to: one bank sends several formats, and each needs its own rule. Nothing
          here needed your approval, and nothing here leaves your Mac. If a rule ever gets something
          wrong, retire it.
        </p>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-xl border border-red-300 bg-red-50 text-sm text-red-800 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {rules !== null && live.length > 0 && (
        <div className="mb-5">
          <StatStrip>
            <StatTile
              icon={<ShieldCheck />}
              label="Rules in use"
              value={live.length}
              hint={`across ${banks.reduce((n, b) => n + b.formats.length, 0)} email formats`}
            />
            <StatTile icon={<Landmark />} label="Banks covered" value={totals.banks} />
            <StatTile
              icon={<Sparkles />}
              label="Corrections avoided"
              value={totals.corrections}
              hint="fields read right automatically"
              tone="good"
            />
            <StatTile
              icon={<Layers />}
              label="Retired"
              value={retired.length}
              hint={retired.length === 0 ? 'none went wrong' : 'no longer used'}
            />
          </StatStrip>
        </div>
      )}

      {showControls && (
        <div className="mb-4 flex items-center gap-2 flex-wrap">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/50">
            Show
          </span>
          {FIELD_FILTERS.map((f) => (
            <button
              key={f}
              type="button"
              onClick={() => setFieldFilter(f)}
              className={cn(
                'text-[12px] font-semibold px-2.5 py-1 rounded-full border transition-colors',
                fieldFilter === f
                  ? 'bg-[#064E3B] text-[#F8E7C9] border-transparent'
                  : 'bg-white/60 text-[#064E3B]/70 border-[#064E3B]/15 hover:border-[#064E3B]/30'
              )}
            >
              {f === 'all' ? 'Everything' : (FIELD_CHIPS[f] ?? f)}
            </button>
          ))}
          <button
            type="button"
            onClick={() => setSortMode((m) => (m === 'default' ? 'weakest' : 'default'))}
            className="ml-auto text-[12px] font-semibold text-[#064E3B] underline underline-offset-2 hover:text-[#053d2f]"
          >
            {sortMode === 'default' ? 'Sorted by bank size' : 'Sorted by least reliable'}
          </button>
        </div>
      )}

      {rules === null ? (
        <div className="p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10 flex items-center gap-2 text-sm text-[#064E3B]/70">
          <Loader2 className="w-4 h-4 animate-spin" /> Loading…
        </div>
      ) : live.length === 0 ? (
        <div className="p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10">
          <h3 className="font-bold text-[15px] text-[#064E3B]">Nothing learned yet</h3>
          <p className="text-[13px] mt-1 text-[#064E3B]/65 leading-relaxed max-w-2xl">
            Rules appear here on their own. Correct a transaction&apos;s merchant, amount or date —
            or run a merchant cleanup above — and Dinero works out which part of that bank&apos;s
            email it read wrongly, then writes a rule so the next one comes through right. You are
            never asked to approve one.
          </p>
        </div>
      ) : banks.length === 0 ? (
        <div className="p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10 text-[13px] text-[#064E3B]/65">
          No rules for that field yet.
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          {banks.map((group, i) => (
            <BankCard
              key={group.bank}
              group={group}
              defaultOpen={i < 2}
              onRetire={setPendingRule}
              revertingId={revertingId}
            />
          ))}
        </div>
      )}

      {retired.length > 0 && (
        <div className="mt-5">
          <button
            type="button"
            onClick={() => setShowRetired((v) => !v)}
            className="text-xs font-semibold text-[#064E3B]/60 hover:text-[#064E3B] flex items-center gap-1"
          >
            <ChevronRight
              className={cn('w-3.5 h-3.5 transition-transform', showRetired && 'rotate-90')}
            />
            {retired.length} retired rule{retired.length === 1 ? '' : 's'}
          </button>
          {showRetired && (
            <div className="mt-2 flex flex-col gap-3 opacity-75">
              {groupRules(retired).map((group) => (
                <BankCard
                  key={group.bank}
                  group={group}
                  defaultOpen
                  onRetire={setPendingRule}
                  revertingId={revertingId}
                />
              ))}
            </div>
          )}
        </div>
      )}

      <div className="mt-8 pt-6 border-t border-[#064E3B]/10">
        <h3 className="font-bold text-[15px] text-[#064E3B] flex items-center gap-2">
          <AtSign className="w-4 h-4" /> Sender bank corrections
        </h3>
        <p className="text-[13px] mt-1 mb-3 text-[#064E3B]/65 leading-relaxed max-w-2xl">
          Domains you have told Dinero belong to a different bank than it guessed. These only change
          which bank a message is filed under — they never let an unrecognised sender through.
        </p>
        {activeOverrides.length === 0 ? (
          <div className="p-4 rounded-xl border border-dashed border-[#064E3B]/15 bg-[#F8E7C9]/40 text-[13px] text-[#064E3B]/60 leading-relaxed">
            None yet. If Dinero ever files a transaction under the wrong bank, open it and report
            the wrong bank from the evidence panel — the domain will be corrected here.
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {activeOverrides.map((o) => (
              <div
                key={o.id}
                className="p-3.5 rounded-xl border bg-white border-[#064E3B]/10 flex items-center justify-between gap-3 flex-wrap"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-mono text-[13px] text-[#064E3B]">{o.domain}</span>
                    <span className="text-[#064E3B]/40">→</span>
                    <span className="font-semibold text-[13px] text-[#064E3B]">{o.bank_name}</span>
                  </div>
                  <p className="text-[11px] mt-1 text-[#064E3B]/50">
                    corrected <RelativeDate iso={o.created_at} />
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setPendingOverride(o)}
                  disabled={revertingId === o.id}
                  className="shrink-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                >
                  {revertingId === o.id ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Undo2 className="w-3.5 h-3.5" />
                  )}
                  <span className="ml-1.5">Remove</span>
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

      <ConfirmDialog
        open={pendingRule !== null}
        onOpenChange={(open) => !open && setPendingRule(null)}
        icon={<Undo2 className="w-5 h-5" aria-hidden="true" />}
        title="Retire this rule?"
        description={
          pendingRule
            ? `Future scans will go back to reading ${FIELD_LABELS[pendingRule.field_name] ?? pendingRule.field_name} from this ${pendingRule.bank_name} email format the way they did before it was learned. Nothing already recorded changes, and the rule stays in your history.`
            : ''
        }
        confirmLabel="Retire rule"
        onConfirm={() => pendingRule && void revertRule(pendingRule)}
      />

      <ConfirmDialog
        open={pendingOverride !== null}
        onOpenChange={(open) => !open && setPendingOverride(null)}
        icon={<Undo2 className="w-5 h-5" aria-hidden="true" />}
        title="Remove this correction?"
        description={
          pendingOverride
            ? `Future mail from ${pendingOverride.domain} will be filed under whichever bank the built-in list says, instead of ${pendingOverride.bank_name}.`
            : ''
        }
        confirmLabel="Remove"
        onConfirm={() => pendingOverride && void revertOverride(pendingOverride)}
      />
    </section>
  );
}
