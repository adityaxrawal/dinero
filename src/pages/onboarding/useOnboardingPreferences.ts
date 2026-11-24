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

  /**
   * Saves the collected preferences; throws if the backend save fails.
   *
   * The backend write also marks onboarding complete, so a failed save leaves
   * the user in onboarding instead of half-onboarded (review P4). The model
   * preference lives only in the database (audit B-3).
   */
  const persist = async () => {
    await API.onboarding.savePreferences({
      timezone,
      spendingLimitMonthly: parseFloat(monthlyLimit) || 0,
      historicalScanMonths: parseInt(SCAN_RANGE, 10) || 3,
      llmModel: llmConfig,
      statementPreference: statementPref,
    });
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
