/**
 * Collects and persists onboarding preferences.
 */
import { useState, useEffect } from 'react';
import { API, type LlmModelInfo } from '@/lib/ipc';

export type StatementPref = 'auto' | 'manual';

const SCAN_RANGE = '3';

/** Collects and persists onboarding preferences. */
export function useOnboardingPreferences() {
  const [timezone, setTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);
  const [monthlyLimit, setMonthlyLimit] = useState('50000');
  const [limitError, setLimitError] = useState<string | null>(null);
  const [statementPref, setStatementPref] = useState<StatementPref>('auto');

  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [llmConfig, setLlmConfig] = useState('gemma4_e4b');

  useEffect(() => {
    API.llm
      .getAvailableModels()
      .then((models) => {
        setAvailableModels(models);
        if (models.length > 0) setLlmConfig(models[0].id);
      })
      .catch((err) => console.error('Failed to fetch LLM model catalog:', err));
  }, []);

  /** Rejects a spending limit that is not a positive number. */
  const validateLimit = () => {
    const parsed = parseFloat(monthlyLimit);
    if (isNaN(parsed) || parsed <= 0) {
      setLimitError('Must be > 0');
      return false;
    }
    setLimitError(null);
    return true;
  };

  /** Saves the collected preferences. */
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
