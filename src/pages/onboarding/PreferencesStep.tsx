/**
 * Preference capture during onboarding.
 */
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { useOnboardingPreferences } from './useOnboardingPreferences';
import StatementPrefPicker from './StatementPrefPicker';

type Prefs = ReturnType<typeof useOnboardingPreferences>;

/** Preference capture during onboarding. */
export default function PreferencesStep({ prefs }: { prefs: Prefs }) {
  return (
    <div className="space-y-4 animate-in fade-in slide-in-from-bottom-4">
      <div className="space-y-2">
        <Label htmlFor="timezone">Timezone</Label>
        <Input
          id="timezone"
          value={prefs.timezone}
          onChange={(e) => prefs.setTimezone(e.target.value)}
          aria-describedby="timezone-hint"
        />
        <p id="timezone-hint" className="text-xs text-muted-foreground">
          Used for aligning transaction dates correctly.
        </p>
      </div>

      <div className="space-y-2">
        <Label htmlFor="limit">Monthly Spending Limit (₹)</Label>
        <Input
          id="limit"
          type="number"
          min="0"
          value={prefs.monthlyLimit}
          onChange={(e) => prefs.setMonthlyLimit(e.target.value)}
          aria-describedby="limit-hint"
        />
        <p id="limit-hint" className="text-xs text-muted-foreground">
          We'll alert you when you approach this limit.
        </p>
        {prefs.limitError && <p className="text-xs text-red-700">{prefs.limitError}</p>}
      </div>

      <StatementPrefPicker value={prefs.statementPref} onChange={prefs.setStatementPref} />

      <div className="space-y-2">
        <Label htmlFor="llm">Local LLM Model</Label>
        <Select value={prefs.llmConfig} onValueChange={prefs.setLlmConfig}>
          <SelectTrigger id="llm" aria-label="Select local LLM model">
            <SelectValue placeholder="Select Model" />
          </SelectTrigger>
          <SelectContent>
            {prefs.availableModels.map((m) => (
              <SelectItem key={m.id} value={m.id}>
                {m.name} ({m.min_ram_gb}GB+ RAM)
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
