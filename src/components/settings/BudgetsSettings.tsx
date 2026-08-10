import type { CategoryBudget } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Bell, Loader2, Save } from 'lucide-react';
import { CategoryBudgetCard, ThresholdToggle } from './budgets/BudgetControls';
import { useBudgetsForm } from './budgets/useBudgetsForm';

const ALERT_THRESHOLDS = [
  { key: 'warn_at_80', label: '80%', description: 'Early warning' },
  { key: 'warn_at_90', label: '90%', description: 'Approaching limit' },
  { key: 'warn_at_100', label: '100%', description: 'Limit reached' },
] as const;

type Thresholds = Record<(typeof ALERT_THRESHOLDS)[number]['key'], boolean>;

function GlobalLimitSection({
  globalLimit,
  onGlobalLimitChange,
  isSaving,
  onSave,
}: {
  globalLimit: string;
  onGlobalLimitChange: (value: string) => void;
  isSaving: boolean;
  onSave: () => void;
}) {
  return (
    <>
      <div className="flex items-start justify-between">
        <div>
          <h2 className="text-xl font-bold flex items-center gap-2 text-[#064E3B]">
            Global Monthly Limit
          </h2>
          <p className="text-sm mt-1 text-[#064E3B]/70">
            Set the total amount you want to spend across all categories per month.
          </p>
        </div>
        <Button
          onClick={onSave}
          disabled={isSaving}
          className="h-9 px-4 font-semibold shrink-0"
          style={{ background: '#064E3B', color: '#F8E7C9' }}
        >
          {isSaving ? (
            <Loader2 className="w-4 h-4 mr-2 animate-spin" />
          ) : (
            <Save className="w-4 h-4 mr-2" />
          )}
          {isSaving ? 'Saving…' : 'Save Changes'}
        </Button>
      </div>

      <div className="flex items-center gap-4 max-w-xs">
        <Label
          htmlFor="global-limit"
          className="shrink-0 text-[13px] font-bold uppercase tracking-wider text-[#064E3B]/60"
        >
          ₹ Limit
        </Label>
        <Input
          id="global-limit"
          type="number"
          min="0"
          value={globalLimit}
          onChange={(e) => onGlobalLimitChange(e.target.value)}
          placeholder="e.g. 60000"
          className="bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus-visible:ring-[#064E3B]"
        />
      </div>
    </>
  );
}

function AlertThresholdsSection({
  thresholds,
  onToggle,
}: {
  thresholds: Thresholds;
  onToggle: (key: keyof Thresholds) => void;
}) {
  return (
    <div>
      <h2 className="text-xl font-bold flex items-center gap-2 mb-1 text-[#064E3B]">
        <Bell className="w-5 h-5" /> Alert Thresholds
      </h2>
      <p className="text-sm mb-6 text-[#064E3B]/70">
        Receive notifications when you cross these percentages of your spending limit.
      </p>

      <div className="flex flex-wrap gap-4">
        {ALERT_THRESHOLDS.map(({ key, label, description }) => (
          <ThresholdToggle
            key={key}
            label={label}
            description={description}
            isActive={thresholds[key]}
            onToggle={() => onToggle(key)}
          />
        ))}
      </div>
    </div>
  );
}

function CategoryBudgetsSection({
  categories,
  onChange,
}: {
  categories: CategoryBudget[];
  onChange: (name: string, value: string) => void;
}) {
  return (
    <div>
      <h2 className="text-xl font-bold mb-1 text-[#064E3B]">Per-Category Budgets</h2>
      <p className="text-sm mb-6 text-[#064E3B]/70">
        Set monthly spending budgets for individual categories. Leave at 0 for no limit.
      </p>

      {categories.length === 0 ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">No categories configured.</p>
      ) : (
        <div
          className="grid gap-4"
          style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}
        >
          {categories.map((cat) => (
            <CategoryBudgetCard key={cat.name} cat={cat} onChange={onChange} />
          ))}
        </div>
      )}
    </div>
  );
}

export default function BudgetsSettings() {
  const {
    loading,
    isSaving,
    globalLimit,
    setGlobalLimit,
    thresholds,
    toggleThreshold,
    categories,
    handleCategoryBudgetChange,
    handleSave,
  } = useBudgetsForm();

  if (loading) {
    return (
      <div className="flex h-40 w-full items-center justify-center">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-12">
      <GlobalLimitSection
        globalLimit={globalLimit}
        onGlobalLimitChange={setGlobalLimit}
        isSaving={isSaving}
        onSave={handleSave}
      />

      <div className="h-px w-full bg-[#064E3B]/10" />

      <AlertThresholdsSection thresholds={thresholds} onToggle={toggleThreshold} />

      <div className="h-px w-full bg-[#064E3B]/10" />

      <CategoryBudgetsSection categories={categories} onChange={handleCategoryBudgetChange} />
    </div>
  );
}
