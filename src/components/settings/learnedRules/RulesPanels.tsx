/**
 * Summary statistics and filter controls for the learned-rules screen.
 */
import { useState } from 'react';
import { AtSign, ChevronRight, Landmark, Layers, Loader2, ShieldCheck, Sparkles, Undo2 } from 'lucide-react';
import type { LearnedRule, SenderBankOverride } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { groupRules } from '../groupRules';
import { RelativeDate, StatStrip, StatTile } from '../SettingsPrimitives';
import BankCard from './BankCard';
import { FIELD_CHIPS } from './labels';
import { FIELD_FILTERS, type useLearnedRules } from './useLearnedRules';

type Rules = ReturnType<typeof useLearnedRules>;

const PANEL = 'p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10';

/** Summary counts across the learned rule set. */
export function RulesStats({ rules }: { rules: Rules }) {
  return (
    <div className="mb-5">
      <StatStrip>
        <StatTile
          icon={<ShieldCheck />}
          label="Rules in use"
          value={rules.live.length}
          hint={`across ${rules.totals.formats} email formats`}
        />
        <StatTile icon={<Landmark />} label="Banks covered" value={rules.totals.banks} />
        <StatTile
          icon={<Sparkles />}
          label="Corrections avoided"
          value={rules.totals.corrections}
          hint="fields read right automatically"
          tone="good"
        />
        <StatTile
          icon={<Layers />}
          label="Retired"
          value={rules.retired.length}
          hint={rules.retired.length === 0 ? 'none went wrong' : 'no longer used'}
        />
      </StatStrip>
    </div>
  );
}

/** Filters the rule list by bank, field and status. */
export function RuleFilters({ rules }: { rules: Rules }) {
  return (
    <div className="mb-4 flex items-center gap-2 flex-wrap">
      <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/50">
        Show
      </span>
      {FIELD_FILTERS.map((f) => (
        <button
          key={f}
          type="button"
          onClick={() => rules.setFieldFilter(f)}
          className={cn(
            'text-[12px] font-semibold px-2.5 py-1 rounded-full border transition-colors',
            rules.fieldFilter === f
              ? 'bg-[#064E3B] text-[#F8E7C9] border-transparent'
              : 'bg-white/60 text-[#064E3B]/70 border-[#064E3B]/15 hover:border-[#064E3B]/30'
          )}
        >
          {f === 'all' ? 'Everything' : (FIELD_CHIPS[f] ?? f)}
        </button>
      ))}
      <button
        type="button"
        onClick={() => rules.setSortMode(rules.sortMode === 'default' ? 'weakest' : 'default')}
        className="ml-auto text-[12px] font-semibold text-[#064E3B] underline underline-offset-2 hover:text-[#053d2f]"
      >
        {rules.sortMode === 'default' ? 'Sorted by bank size' : 'Sorted by least reliable'}
      </button>
    </div>
  );
}

/** The live rules, grouped by bank. */
export function RulesList({
  rules,
  onRetire,
}: {
  rules: Rules;
  onRetire: (rule: LearnedRule) => void;
}) {
  if (rules.rules === null) {
    return (
      <div className={`${PANEL} flex items-center gap-2 text-sm text-[#064E3B]/70`}>
        <Loader2 className="w-4 h-4 animate-spin" /> Loading…
      </div>
    );
  }
  if (rules.live.length === 0) {
    return (
      <div className={PANEL}>
        <h3 className="font-bold text-[15px] text-[#064E3B]">Nothing learned yet</h3>
        <p className="text-[13px] mt-1 text-[#064E3B]/65 leading-relaxed max-w-2xl">
          Rules appear here on their own. Correct a transaction&apos;s merchant, amount or date — or
          run a merchant cleanup above — and Dinero works out which part of that bank&apos;s email
          it read wrongly, then writes a rule so the next one comes through right. You are never
          asked to approve one.
        </p>
      </div>
    );
  }
  if (rules.banks.length === 0) {
    return (
      <div className={`${PANEL} text-[13px] text-[#064E3B]/65`}>No rules for that field yet.</div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {rules.banks.map((group, i) => (
        <BankCard
          key={group.bank}
          group={group}
          defaultOpen={i < 2}
          onRetire={onRetire}
          revertingId={rules.revertingId}
        />
      ))}
    </div>
  );
}

/**
 * Rules no longer in use.
 *
 * Kept visible rather than hidden, so a rule that stopped working after a bank
 * changed its template can be found and understood instead of silently vanishing.
 */
export function RetiredRules({
  retired,
  revertingId,
  onRetire,
}: {
  retired: LearnedRule[];
  revertingId: string | null;
  onRetire: (rule: LearnedRule) => void;
}) {
  const [showRetired, setShowRetired] = useState(false);

  return (
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
              onRetire={onRetire}
              revertingId={revertingId}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** One sender-to-bank override, with its deactivate control. */
function OverrideRow({
  override,
  revertingId,
  onRemove,
}: {
  override: SenderBankOverride;
  revertingId: string | null;
  onRemove: () => void;
}) {
  const isBusy = revertingId === override.id;
  return (
    <div className="p-3.5 rounded-xl border bg-white border-[#064E3B]/10 flex items-center justify-between gap-3 flex-wrap">
      <div className="min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-mono text-[13px] text-[#064E3B]">{override.domain}</span>
          <span className="text-[#064E3B]/40">→</span>
          <span className="font-semibold text-[13px] text-[#064E3B]">{override.bank_name}</span>
        </div>
        <p className="text-[11px] mt-1 text-[#064E3B]/50">
          corrected <RelativeDate iso={override.created_at} />
        </p>
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={onRemove}
        disabled={isBusy}
        className="shrink-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
      >
        {isBusy ? (
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
        ) : (
          <Undo2 className="w-3.5 h-3.5" />
        )}
        <span className="ml-1.5">Remove</span>
      </Button>
    </div>
  );
}

/**
 * Manual sender-domain overrides.
 *
 * The user-facing correction for misidentified banks. Overrides are deactivated
 * rather than deleted, preserving the record of what was configured.
 */
export function SenderOverrides({
  overrides,
  revertingId,
  onRemove,
}: {
  overrides: SenderBankOverride[];
  revertingId: string | null;
  onRemove: (o: SenderBankOverride) => void;
}) {
  return (
    <div className="mt-8 pt-6 border-t border-[#064E3B]/10">
      <h3 className="font-bold text-[15px] text-[#064E3B] flex items-center gap-2">
        <AtSign className="w-4 h-4" /> Sender bank corrections
      </h3>
      <p className="text-[13px] mt-1 mb-3 text-[#064E3B]/65 leading-relaxed max-w-2xl">
        Domains you have told Dinero belong to a different bank than it guessed. These only change
        which bank a message is filed under — they never let an unrecognised sender through.
      </p>
      {overrides.length === 0 ? (
        <div className="p-4 rounded-xl border border-dashed border-[#064E3B]/15 bg-[#F8E7C9]/40 text-[13px] text-[#064E3B]/60 leading-relaxed">
          None yet. If Dinero ever files a transaction under the wrong bank, open it and report the
          wrong bank from the evidence panel — the domain will be corrected here.
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {overrides.map((o) => (
            <OverrideRow
              key={o.id}
              override={o}
              revertingId={revertingId}
              onRemove={() => onRemove(o)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
