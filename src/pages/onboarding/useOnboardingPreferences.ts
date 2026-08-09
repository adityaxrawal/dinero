import { useState, useEffect } from 'react';
import { API, type LlmModelInfo } from '@/lib/ipc';

export type StatementPref = 'auto' | 'manual';

// TASK-FE-006: the real scan range is now chosen on HistoricalScanScreen
// (a date-range picker, not a months count) after Gmail connects — this
// fixed default only feeds the separate `historicalScanMonths` backend
// preference field, unrelated to what actually gets scanned.
const SCAN_RANGE = '3';

export function useOnboardingPreferences() {
  const [timezone, setTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);
  const [monthlyLimit, setMonthlyLimit] = useState('50000');
  const [limitError, setLimitError] = useState<string | null>(null);
  const [statementPref, setStatementPref] = useState<StatementPref>('auto');

  // Doc 16 §12.3: the 5-tier model catalog, fetched from the backend — not
  // hardcoded here, so this can never drift from src-tauri's own list again.
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [llmConfig, setLlmConfig] = useState('gemma4_e4b');

  useEffect(() => {
    API.llm
      .getAvailableModels()
      .then((models) => {
        setAvailableModels(models);
        // Default to the lowest-tier (broadest-compatibility) model.
        if (models.length > 0) setLlmConfig(models[0].id);
      })
      .catch((err) => console.error('Failed to fetch LLM model catalog:', err));
  }, []);

  /** True when step 1's spending limit is a usable number. */
  const validateLimit = () => {
    const parsed = parseFloat(monthlyLimit);
    if (isNaN(parsed) || parsed <= 0) {
      setLimitError('Must be > 0');
      return false;
    }
    setLimitError(null);
    return true;
  };

  // G19 fix: previously these choices only lived in browser localStorage —
  // never persisted to `local_profile`, so they didn't survive a
  // reinstall/reset and `monthlyLimit` in particular never reached the same
  // row Settings → Spending Limits reads from. Best-effort: a persistence
  // hiccup here shouldn't block the user from finishing onboarding, since
  // Settings still offers a normal way to set these afterward.
  const persist = async () => {
    localStorage.setItem('dinero_onboarded', 'true');
    localStorage.setItem('dinero_monthly_limit', monthlyLimit);
    localStorage.setItem('dinero_scan_range', SCAN_RANGE);
    localStorage.setItem('llm_model', llmConfig);
    localStorage.setItem('dinero_statement_pref', statementPref);
    try {
      await API.onboarding.savePreferences({
        timezone,
        spendingLimitMonthly: parseFloat(monthlyLimit) || 0,
        historicalScanMonths: parseInt(SCAN_RANGE, 10) || 3,
        llmModel: llmConfig,
        statementPreference: statementPref,
      });
    } catch (err) {
      console.error('Failed to persist onboarding preferences to backend:', err);
    }
  };

  return {
    timezone,
    setTimezone,
    monthlyLimit,
    setMonthlyLimit,
    limitError,
    statementPref,
    setStatementPref,
    availableModels,
    llmConfig,
    setLlmConfig,
    validateLimit,
    persist,
  };
}
