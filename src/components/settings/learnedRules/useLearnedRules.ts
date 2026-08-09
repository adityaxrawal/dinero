import { useState, useEffect, useCallback, useMemo } from 'react';
import { API, type LearnedRule, type SenderBankOverride } from '@/lib/ipc';
import { toast } from '@/hooks/use-toast';
import { errorMessage } from '@/lib/utils';
import { groupRules } from '../groupRules';
import { FIELD_LABELS } from './labels';

export const FIELD_FILTERS = ['all', 'merchant', 'amount', 'event_time'] as const;
type FieldFilter = (typeof FIELD_FILTERS)[number];
type SortMode = 'default' | 'weakest';

export function useLearnedRules() {
  const [rules, setRules] = useState<LearnedRule[] | null>(null);
  const [overrides, setOverrides] = useState<SenderBankOverride[] | null>(null);
  const [revertingId, setRevertingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fieldFilter, setFieldFilter] = useState<FieldFilter>('all');
  const [sortMode, setSortMode] = useState<SortMode>('default');

  const load = useCallback(async () => {
    try {
      const [r, o] = await Promise.all([API.learnedRules.list(), API.senderOverrides.list()]);
      setRules(r);
      setOverrides(o);
    } catch (err: unknown) {
      setError(errorMessage(err));
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
      setError(errorMessage(err));
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
      setError(errorMessage(err));
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
    const filtered = fieldFilter === 'all' ? live : live.filter((r) => r.field_name === fieldFilter);
    const grouped = groupRules(filtered);
    return sortMode === 'weakest'
      ? [...grouped].sort((a, b) => a.confidence - b.confidence)
      : grouped;
  }, [live, fieldFilter, sortMode]);

  const totals = useMemo(
    () => ({
      banks: new Set(live.map((r) => r.bank_name)).size,
      corrections: live.reduce((sum, r) => sum + r.success_count, 0),
      formats: banks.reduce((n, b) => n + b.formats.length, 0),
    }),
    [live, banks]
  );

  return {
    rules,
    live,
    retired,
    banks,
    totals,
    error,
    revertingId,
    fieldFilter,
    setFieldFilter,
    sortMode,
    setSortMode,
    /** Only worth showing controls once the list is long enough to need them. */
    showControls: live.length > 6,
    activeOverrides: overrides?.filter((o) => o.status === 'active') ?? [],
    revertRule,
    revertOverride,
  };
}
