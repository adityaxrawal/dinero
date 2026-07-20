import { useState, useEffect, useCallback } from 'react';
import { API, SpendingLimits as SpendingLimitsData, CategoryBudget } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Bell, Loader2, Save } from 'lucide-react';
import { useToast } from '@/hooks/use-toast';
import { cn } from '@/lib/utils';

export default function BudgetsSettings() {
  const { toast } = useToast();
  const [loading, setLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  // Editable local state
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
      setGlobalLimit(data.global_limit ? (data.global_limit / 100).toString() : '');
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
    setCategories((prev) =>
      prev.map((c) => (c.name === name ? { ...c, budget } : c)),
    );
  };

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
    
    if (parsedLimit > 999999999999) {
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
      toast({ title: 'Spending Limits Saved', description: 'Your spending limits have been updated.' });
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

  if (loading) {
    return (
      <div className="flex h-40 w-full items-center justify-center">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-12">
      <div className="flex items-start justify-between">
        <div>
          <h2 className="text-xl font-bold flex items-center gap-2 text-[#064E3B]">Global Monthly Limit</h2>
          <p className="text-sm mt-1 text-[#064E3B]/70">Set the total amount you want to spend across all categories per month.</p>
        </div>
        <Button onClick={handleSave} disabled={isSaving} className="h-9 px-4 font-semibold shrink-0" style={{ background: '#064E3B', color: '#F8E7C9' }}>
          {isSaving ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <Save className="w-4 h-4 mr-2" />}
          {isSaving ? 'Saving…' : 'Save Changes'}
        </Button>
      </div>

      <div className="flex items-center gap-4 max-w-xs">
        <Label htmlFor="global-limit" className="shrink-0 text-[13px] font-bold uppercase tracking-wider text-[#064E3B]/60">₹ Limit</Label>
        <Input
          id="global-limit"
          type="number"
          min="0"
          value={globalLimit}
          onChange={(e) => setGlobalLimit(e.target.value)}
          placeholder="e.g. 60000"
          className="bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus-visible:ring-[#064E3B]"
        />
      </div>

      <div className="h-px w-full bg-[#064E3B]/10" />

      <div>
        <h2 className="text-xl font-bold flex items-center gap-2 mb-1 text-[#064E3B]">
          <Bell className="w-5 h-5" /> Alert Thresholds
        </h2>
        <p className="text-sm mb-6 text-[#064E3B]/70">Receive notifications when you cross these percentages of your spending limit.</p>
        
        <div className="flex flex-wrap gap-4">
          {(
            [
              { key: 'warn_at_80', label: '80%', description: 'Early warning' },
              { key: 'warn_at_90', label: '90%', description: 'Approaching limit' },
              { key: 'warn_at_100', label: '100%', description: 'Limit reached' },
            ] as const
          ).map(({ key, label, description }) => {
            const isActive = thresholds[key];
            return (
              <button
                key={key}
                type="button"
                onClick={() => setThresholds((prev) => ({ ...prev, [key]: !prev[key] }))}
                className={cn(
                  'relative flex flex-col items-center px-6 py-5 rounded-xl text-[13px] transition-all duration-200 outline-none',
                  'min-w-[120px] border',
                  isActive
                    ? 'border-[#064E3B] bg-[#064E3B]/5 ring-1 ring-[#064E3B]/20'
                    : 'border-[#064E3B]/20 bg-[#F8E7C9]/50 hover:border-[#064E3B]/30 hover:bg-[#064E3B]/5'
                )}
              >
                <span className={cn('text-2xl font-bold transition-colors', isActive ? 'text-[#064E3B]' : 'text-[#064E3B]/70')}>
                  {label}
                </span>
                <span className="text-[12px] font-medium mt-1 text-[#064E3B]/70">{description}</span>
                <span className={cn(
                  'mt-3 px-2.5 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider',
                  isActive ? 'bg-[#064E3B]/10 text-[#064E3B]' : 'bg-[#064E3B]/5 text-[#064E3B]/60 border border-[#064E3B]/10'
                )}>
                  {isActive ? 'ON' : 'OFF'}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="h-px w-full bg-[#064E3B]/10" />

      <div>
        <h2 className="text-xl font-bold mb-1 text-[#064E3B]">Per-Category Budgets</h2>
        <p className="text-sm mb-6 text-[#064E3B]/70">Set monthly spending budgets for individual categories. Leave at 0 for no limit.</p>
        
        {categories.length === 0 ? (
          <p className="text-[13px] font-medium text-[#064E3B]/70">No categories configured.</p>
        ) : (
          <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}>
            {categories.map((cat) => (
              <div key={cat.name} className="space-y-3 p-5 rounded-xl border border-[#064E3B]/10 bg-[#064E3B]/5">
                <Label htmlFor={`cat-${cat.name}`} className="text-[14px] font-bold text-[#064E3B]">{cat.name}</Label>
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-medium text-[#064E3B]/70 shrink-0">₹</span>
                  <Input
                    id={`cat-${cat.name}`}
                    type="number"
                    min="0"
                    value={cat.budget === 0 ? '' : cat.budget}
                    placeholder="No limit"
                    onChange={(e) => handleCategoryBudgetChange(cat.name, e.target.value)}
                    className="bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus-visible:ring-[#064E3B]"
                  />
                </div>
                {cat.budget > 0 && (
                  <p className="text-[12px] font-semibold text-[#064E3B]/60">₹ {cat.budget.toLocaleString()} / month</p>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
