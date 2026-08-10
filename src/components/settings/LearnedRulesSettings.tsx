/**
 * Lists the extraction rules the app inferred from corrections, and allows reverting them.
 *
 * Makes the learning loop inspectable rather than opaque -- a rule that started
 * producing wrong values can be found and removed instead of silently degrading
 * every future extraction from that bank.
 */
import { useState } from 'react';
import { AlertTriangle, GraduationCap, Undo2 } from 'lucide-react';
import type { LearnedRule, SenderBankOverride } from '@/lib/ipc';
import { ConfirmDialog } from './SettingsPrimitives';
import SectionHeading from './SectionHeading';
import { useLearnedRules } from './learnedRules/useLearnedRules';
import { FIELD_LABELS } from './learnedRules/labels';
import {
  RulesStats,
  RuleFilters,
  RulesList,
  RetiredRules,
  SenderOverrides,
} from './learnedRules/RulesPanels';

const BLURB =
  'Every time you correct a merchant, amount or date, Dinero works out the rule behind the mistake and checks it against your own history before using it — so the same bank’s next email gets read correctly without you doing anything. Rules are grouped by the email layout they apply to: one bank sends several formats, and each needs its own rule. Nothing here needed your approval, and nothing here leaves your Mac. If a rule ever gets something wrong, retire it.';

/** Lists learned extraction rules and allows reverting them. */
export default function LearnedRulesSettings() {
  const rules = useLearnedRules();
  const [pendingRule, setPendingRule] = useState<LearnedRule | null>(null);
  const [pendingOverride, setPendingOverride] = useState<SenderBankOverride | null>(null);

  return (
    <section>
      <SectionHeading icon={GraduationCap} title="What Dinero Has Learned" description={BLURB} />

      {rules.error && (
        <div className="mb-4 p-4 rounded-xl border border-red-300 bg-red-50 text-sm text-red-800 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{rules.error}</span>
        </div>
      )}

      {rules.rules !== null && rules.live.length > 0 && <RulesStats rules={rules} />}

      {rules.showControls && <RuleFilters rules={rules} />}

      <RulesList rules={rules} onRetire={setPendingRule} />

      {rules.retired.length > 0 && (
        <RetiredRules
          retired={rules.retired}
          revertingId={rules.revertingId}
          onRetire={setPendingRule}
        />
      )}

      <SenderOverrides
        overrides={rules.activeOverrides}
        revertingId={rules.revertingId}
        onRemove={setPendingOverride}
      />

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
        onConfirm={() => pendingRule && void rules.revertRule(pendingRule)}
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
        onConfirm={() => pendingOverride && void rules.revertOverride(pendingOverride)}
      />
    </section>
  );
}
