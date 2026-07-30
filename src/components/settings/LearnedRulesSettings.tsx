import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  GraduationCap,
  Loader2,
  Undo2,
  AlertTriangle,
  ChevronRight,
  Mail,
  FileText,
} from 'lucide-react';
import { API, type LearnedRule, type SenderBankOverride } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

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
 * So the pattern is deliberately *not* the headline. Each row says what the
 * rule does in words; the regex sits behind a disclosure for the rare case
 * someone is debugging. The one judgment left to a human is "this rule is
 * misbehaving, retire it", which is the revert button.
 */

/** Human-readable description of what a rule actually does. */
function describeRule(rule: LearnedRule): string {
  const source = rule.source_type === 'email' ? 'email alerts' : 'PDF statements';
  const field =
    {
      merchant: 'the merchant name',
      amount: 'the amount',
      event_time: 'the transaction date',
      reference_id: 'the reference number',
      balance: 'the closing balance',
      last4: 'the card/account digits',
      direction: 'whether it is money in or out',
      currency: 'the currency',
    }[rule.field_name] ?? rule.field_name;

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
        'text-[11px] font-semibold px-2 py-0.5 rounded-full border shrink-0',
        STATUS_STYLES[status] ?? STATUS_STYLES.inactive
      )}
    >
      {STATUS_LABELS[status] ?? status}
    </span>
  );
}

function RuleRow({
  rule,
  onRevert,
  isReverting,
}: {
  rule: LearnedRule;
  onRevert: (id: string) => void;
  isReverting: boolean;
}) {
  const [showPattern, setShowPattern] = useState(false);
  const isLive = rule.status !== 'inactive' && rule.status !== 'flagged';
  const pattern = rule.rule_payload_json.regex ?? rule.rule_payload_json.override_value ?? '';

  return (
    <div
      className={cn(
        'p-4 rounded-xl border',
        isLive ? 'bg-white border-[#064E3B]/10' : 'bg-neutral-50 border-neutral-200 opacity-70'
      )}
    >
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            {rule.source_type === 'email' ? (
              <Mail className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/50" />
            ) : (
              <FileText className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/50" />
            )}
            <span className="font-semibold text-[14px] text-[#064E3B]">{describeRule(rule)}</span>
            <StatusBadge status={rule.status} />
          </div>
          <p className="text-xs mt-1 text-[#064E3B]/60">{describeOrigin(rule)}</p>
          <p className="text-xs mt-1 text-[#064E3B]/50">
            Worked {rule.success_count} time{rule.success_count === 1 ? '' : 's'}
            {rule.failure_count > 0 && `, was corrected again ${rule.failure_count}×`}
            {' · '}
            {Math.round(rule.confidence * 100)}% confidence
          </p>
        </div>

        {isLive && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => onRevert(rule.id)}
            disabled={isReverting}
            className="shrink-0"
          >
            {isReverting ? (
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
        onClick={() => setShowPattern((v) => !v)}
        className="mt-2 text-[11px] text-[#064E3B]/50 hover:text-[#064E3B]/80 flex items-center gap-1"
      >
        <ChevronRight
          className={cn('w-3 h-3 transition-transform', showPattern && 'rotate-90')}
        />
        Technical detail
      </button>
      {showPattern && (
        <pre className="mt-2 p-2 rounded-lg bg-[#064E3B]/5 text-[11px] font-mono overflow-x-auto text-[#064E3B]/80">
          {pattern}
        </pre>
      )}
    </div>
  );
}

export default function LearnedRulesSettings() {
  const [rules, setRules] = useState<LearnedRule[] | null>(null);
  const [overrides, setOverrides] = useState<SenderBankOverride[] | null>(null);
  const [revertingId, setRevertingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showRetired, setShowRetired] = useState(false);

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

  const revertRule = async (id: string) => {
    if (
      !confirm(
        'Retire this rule? Future scans will go back to reading that field the way they did ' +
          'before it was learned. Nothing already recorded changes.'
      )
    )
      return;
    setRevertingId(id);
    setError(null);
    try {
      await API.learnedRules.revert(id);
      await load();
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setRevertingId(null);
    }
  };

  const revertOverride = async (o: SenderBankOverride) => {
    if (
      !confirm(
        `Remove the correction for ${o.domain}? Future mail from that domain will be filed ` +
          `under whichever bank the built-in list says, instead of ${o.bank_name}.`
      )
    )
      return;
    setRevertingId(o.id);
    setError(null);
    try {
      await API.senderOverrides.revert(o.id);
      await load();
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

  const byBank = useMemo(() => {
    const groups = new Map<string, LearnedRule[]>();
    for (const r of live) {
      const existing = groups.get(r.bank_name);
      if (existing) existing.push(r);
      else groups.set(r.bank_name, [r]);
    }
    return [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [live]);

  const activeOverrides = overrides?.filter((o) => o.status === 'active') ?? [];

  return (
    <section>
      <div className="mb-6">
        <h2 className="text-xl font-bold flex items-center gap-2">
          <GraduationCap className="w-5 h-5" /> What Dinero Has Learned
        </h2>
        <p className="text-sm mt-1 text-[#064E3B]/70">
          Every time you correct a merchant, amount or date, Dinero works out the rule behind the
          mistake and checks it against your own history before using it — so the same bank&apos;s
          next email gets read correctly without you doing anything. Nothing here needed your
          approval, and nothing here leaves your Mac. If a rule ever gets something wrong, retire
          it.
        </p>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-xl border border-red-300 bg-red-50 text-sm text-red-800 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {rules === null ? (
        <div className="p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10 flex items-center gap-2 text-sm text-[#064E3B]/70">
          <Loader2 className="w-4 h-4 animate-spin" /> Loading…
        </div>
      ) : live.length === 0 ? (
        <div className="p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10">
          <h3 className="font-bold text-[15px] text-[#064E3B]">Nothing learned yet</h3>
          <p className="text-xs mt-1 text-[#064E3B]/60">
            Correct a transaction&apos;s merchant, amount or date and the matching rule will be
            written here automatically.
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-6">
          {byBank.map(([bank, bankRules]) => (
            <div key={bank}>
              <h3 className="font-bold text-[15px] text-[#064E3B] mb-2">{bank}</h3>
              <div className="flex flex-col gap-2">
                {bankRules.map((r) => (
                  <RuleRow
                    key={r.id}
                    rule={r}
                    onRevert={revertRule}
                    isReverting={revertingId === r.id}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {retired.length > 0 && (
        <div className="mt-6">
          <button
            type="button"
            onClick={() => setShowRetired((v) => !v)}
            className="text-xs text-[#064E3B]/60 hover:text-[#064E3B] flex items-center gap-1"
          >
            <ChevronRight
              className={cn('w-3.5 h-3.5 transition-transform', showRetired && 'rotate-90')}
            />
            {retired.length} retired rule{retired.length === 1 ? '' : 's'}
          </button>
          {showRetired && (
            <div className="mt-2 flex flex-col gap-2">
              {retired.map((r) => (
                <RuleRow key={r.id} rule={r} onRevert={revertRule} isReverting={false} />
              ))}
            </div>
          )}
        </div>
      )}

      <div className="mt-8 pt-6 border-t border-[#064E3B]/10">
        <h3 className="font-bold text-[15px] text-[#064E3B]">Sender bank corrections</h3>
        <p className="text-xs mt-1 mb-3 text-[#064E3B]/60">
          Domains you have told Dinero belong to a different bank than it guessed. These only change
          which bank a message is filed under — they never let an unrecognised sender through.
        </p>
        {activeOverrides.length === 0 ? (
          <p className="text-xs text-[#064E3B]/50">None.</p>
        ) : (
          <div className="flex flex-col gap-2">
            {activeOverrides.map((o) => (
              <div
                key={o.id}
                className="p-3 rounded-xl border bg-white border-[#064E3B]/10 flex items-center justify-between gap-3 flex-wrap"
              >
                <div className="min-w-0">
                  <span className="font-mono text-[13px] text-[#064E3B]">{o.domain}</span>
                  <span className="mx-2 text-[#064E3B]/40">→</span>
                  <span className="font-semibold text-[13px] text-[#064E3B]">{o.bank_name}</span>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => revertOverride(o)}
                  disabled={revertingId === o.id}
                  className="shrink-0"
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
    </section>
  );
}
