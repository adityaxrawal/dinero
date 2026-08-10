import { useState, useEffect, useCallback } from 'react';
import { API, SpendingLimits as SpendingLimitsData, CategoryBudget } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';

/** Rejected before the round trip: the backend stores paise in an i64, and a
 *  limit this large is a typo rather than an intent. */
const MAX_GLOBAL_LIMIT = 999999999999;

/**
 * Load/edit/save state for the spending-limits form. Split out of
 * `BudgetsSettings` so the validation rules below are reachable without
 * rendering the settings page.
 */
export function useBudgetsForm() {
  const { toast } = useToast();
  const [loading, setLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  const [globalLimit, setGlobalLimit] = useState('');
  const [thresholds, setThresholds] = useState({
    warn_at_80: true,
    warn_at_90: true,
    warn_at_100: true,
  });
  const [categories, setCategories] = useState<CategoryBudget[]>([]);

  const loadLimits = useCallback(async () => {
    setLoading(true);
    try {
      const data = await API.spendingLimits.get();
      setGlobalLimit(data.global_limit ? data.global_limit.toString() : '');
      setThresholds({
        warn_at_80: data.thresholds.warn_at_80,
        warn_at_90: data.thresholds.warn_at_90,
        warn_at_100: data.thresholds.warn_at_100,
      });
      setCategories(data.categories);
    } catch (e) {
      console.error('Failed to load spending limits', e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadLimits();
  }, [loadLimits]);

  const handleCategoryBudgetChange = (name: string, value: string) => {
    const budget = parseFloat(value) || 0;
    setCategories((prev) => prev.map((c) => (c.name === name ? { ...c, budget } : c)));
  };

  const toggleThreshold = (key: keyof typeof thresholds) =>
    setThresholds((prev) => ({ ...prev, [key]: !prev[key] }));

  const handleSave = async () => {
    const parsedLimit = parseFloat(globalLimit);
    if (isNaN(parsedLimit) || parsedLimit < 0) {
      toast({
        variant: 'destructive',
        title: 'Invalid limit',
        description: 'Global limit must be a positive number.',
      });
      return;
    }

    if (parsedLimit > MAX_GLOBAL_LIMIT) {
      toast({
        variant: 'destructive',
        title: 'Too large',
        description: 'Limit is too large.',
      });
      return;
    }

    setIsSaving(true);
    try {
      const updated: SpendingLimitsData = {
        global_limit: parsedLimit,
        thresholds,
        categories,
      };
      await API.spendingLimits.update(updated);
      toast({
        title: 'Spending Limits Saved',
        description: 'Your spending limits have been updated.',
      });
    } catch (e: unknown) {
      toast({
        variant: 'destructive',
        title: 'Save Failed',
        description: e instanceof Error ? e.message : 'Could not save spending limits.',
      });
    } finally {
      setIsSaving(false);
    }
  };

  return {
    loading,
    isSaving,
    globalLimit,
    setGlobalLimit,
    thresholds,
    toggleThreshold,
    categories,
    handleCategoryBudgetChange,
    handleSave,
  };
}
