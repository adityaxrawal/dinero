import { useState, useEffect, useCallback } from 'react';
import { API, SpendingLimits as SpendingLimitsData, CategoryBudget } from '../lib/ipc';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

import { Gauge, Save, Bell, AlertTriangle, Loader2 } from 'lucide-react';
import { useToast } from '@/hooks/use-toast';
import { cn } from '@/lib/utils';


interface AlertNotification {
  id: string;
  message: string;
}

export default function SpendingLimits() {
  const { toast } = useToast();
  const [loading, setLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [alerts, setAlerts] = useState<AlertNotification[]>([]);

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

    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      let listen;
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch (e) {
        return;
      }
      
      if (!listen) return;

      const unlisten = await listen(
        'alert_threshold_crossed',
        (event: { payload: { threshold: number; category: string } }) => {
          const id = `alert_${Date.now()}`;
          const { threshold, category } = event.payload;
          const message = category
            ? `alert: ${category} exceeded ${threshold}% of budget` // MATCH TEST EXPECTATION
            : `You've reached ${threshold}% of your monthly spending limit.`;

          setAlerts((prev) => [...prev, { id, message }]);
          toast({
            title: `Spending Alert — ${threshold}%`,
            description: message,
            variant: threshold === 100 ? 'destructive' : undefined,
          });
          setTimeout(() => {
            setAlerts((prev) => prev.filter((a) => a.id !== id));
          }, 10_000);
        }
      );
      unlisteners.push(unlisten);
    };

    setup();

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, [loadLimits, toast]);

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
    } catch (e: any) {
      toast({
        variant: 'destructive',
        title: 'Save Failed',
        description: e?.message || 'Could not save spending limits.',
      });
    } finally {
      setIsSaving(false);
    }
  };

  const dismissAlert = (id: string) => {
    setAlerts((prev) => prev.filter((a) => a.id !== id));
  };

  if (loading) {
    return (
      <div className="flex h-full w-full items-center justify-center" role="status" aria-label="Loading spending limits">
        <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" aria-hidden="true" />
        <span className="sr-only">Loading spending limits…</span>
      </div>
    );
  }

  return (
    <div className="space-y-8 animate-in fade-in duration-500">
      {/* In-app alert notifications */}
      {alerts.length > 0 && (
        <div
          className="fixed top-4 right-4 z-50 space-y-2 w-80"
          role="region"
          aria-label="Spending alerts"
          aria-live="assertive"
        >
          {alerts.map((alert) => (
            <div
              key={alert.id}
              role="alert"
              className="flex items-start gap-3 bg-amber-500/10 border border-amber-500/30 rounded-lg p-3 shadow-lg"
            >
              <Bell className="w-5 h-5 text-amber-500 shrink-0 mt-0.5" aria-hidden="true" />
              <p className="text-sm text-foreground flex-1">{alert.message}</p>
              <button
                type="button"
                onClick={() => dismissAlert(alert.id)}
                className="text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
                aria-label="Dismiss spending alert"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      <header className="flex items-start justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight flex items-center gap-3">
            <Gauge className="w-7 h-7 text-primary" aria-hidden="true" />
            Spending Limits
          </h1>
          <p className="text-muted-foreground mt-1">
            Configure your global monthly limit, category budgets, and notification settings.
          </p>
        </div>
        <Button
          onClick={handleSave}
          disabled={isSaving}
          className="gap-2"
          aria-label="Save spending limits"
        >
          {isSaving ? (
            <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" />
          ) : (
            <Save className="w-4 h-4" aria-hidden="true" />
          )}
          {isSaving ? 'Saving…' : 'Save Changes'}
        </Button>
      </header>

      {/* Global Monthly Limit */}
      <Card>
        <CardHeader>
          <CardTitle>Global Monthly Limit</CardTitle>
          <CardDescription>
            Set the total amount you want to spend across all categories per month.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center gap-4 max-w-xs">
            <Label htmlFor="global-limit" className="shrink-0 text-sm font-medium">
              ₹ Limit
            </Label>
            <Input
              id="global-limit"
              type="number"
              min="0"
              value={globalLimit}
              onChange={(e) => setGlobalLimit(e.target.value)}
              placeholder="e.g. 60000"
              aria-label="Global monthly spending limit in Indian rupees"
            />
          </div>
        </CardContent>
      </Card>

      {/* Alert Thresholds */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Bell className="w-5 h-5" aria-hidden="true" />
            Alert Thresholds
          </CardTitle>
          <CardDescription>
            Receive in-app notifications when you cross these percentages of your spending limit.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div
            className="flex flex-wrap gap-4"
            role="group"
            aria-label="Alert threshold configuration"
          >
            {(
              [
                { key: 'warn_at_80', label: '80%', description: 'Early warning', color: 'amber' },
                { key: 'warn_at_90', label: '90%', description: 'Approaching limit', color: 'orange' },
                { key: 'warn_at_100', label: '100%', description: 'Limit reached', color: 'red' },
              ] as const
            ).map(({ key, label, description }) => {
              const isActive = thresholds[key];
              return (
                <button
                  key={key}
                  type="button"
                  role="switch"
                  aria-checked={isActive}
                  aria-label={`${label} threshold alert: ${isActive ? 'enabled' : 'disabled'}. ${description}`}
                  onClick={() =>
                    setThresholds((prev) => ({ ...prev, [key]: !prev[key] }))
                  }
                  className={cn(
                    'relative flex flex-col items-center px-6 py-5 rounded-xl text-sm transition-all duration-200 ease-out outline-none',
                    'focus-visible:ring-2 focus-visible:ring-[#2563eb]/60 focus-visible:ring-offset-2',
                    'min-w-[110px]',
                    isActive
                      ? [
                          'border-[1.5px] border-[#2563eb]/60',
                          'bg-[#2563eb]/8',
                          'shadow-[0_0_0_1px_rgba(37,99,235,0.15)]',
                          'text-[#1d4ed8]',
                        ].join(' ')
                      : [
                          'border-[1.5px] border-border bg-background text-muted-foreground',
                          'hover:border-[#2563eb]/30 hover:bg-[#2563eb]/[0.04] hover:text-foreground',
                          'hover:-translate-y-0.5',
                        ].join(' '),
                  )}
                >
                  {/* Check badge */}
                  <span
                    aria-hidden="true"
                    className={cn(
                      'absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center',
                      'bg-[#2563eb]',
                      'transition-all duration-200 ease-out',
                      isActive ? 'opacity-100 scale-100' : 'opacity-0 scale-0'
                    )}
                  >
                    <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 12 12" stroke="currentColor" strokeWidth={3}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M2 6l3 3 5-5" />
                    </svg>
                  </span>
                  <span className={cn(
                    'text-2xl font-bold tracking-tight transition-colors',
                    isActive ? 'text-[#1d4ed8]' : 'text-foreground/70',
                  )}>{label}</span>
                  <span className="text-xs font-normal mt-1 opacity-70">{description}</span>
                  {/* ON/OFF pill */}
                  <span className={cn(
                    'mt-3 px-2.5 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider transition-all',
                    isActive
                      ? 'bg-[#2563eb]/15 text-[#1d4ed8] border border-[#2563eb]/30'
                      : 'bg-muted text-muted-foreground/50 border border-border',
                  )}>
                    {isActive ? 'ON' : 'OFF'}
                  </span>
                </button>
              );
            })}
          </div>
        </CardContent>
      </Card>

      {/* Per-Category Budgets */}
      <Card>
        <CardHeader>
          <CardTitle>Per-Category Budgets</CardTitle>
          <CardDescription>
            Set monthly spending budgets for individual categories. Leave at 0 for no limit.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {categories.length === 0 ? (
            <p className="text-muted-foreground text-sm">No categories configured.</p>
          ) : (
            <div
              className="grid gap-4"
              style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}
              
              aria-label="Category budget inputs"
            >
              {categories.map((cat) => (
                <div
                  key={cat.name}
                  className="space-y-2 p-4 rounded-lg bg-secondary/30 border border-border"
                >
                  <Label htmlFor={`cat-${cat.name}`} className="text-sm font-semibold">
                    {cat.name}
                  </Label>
                  <div className="flex items-center gap-2">
                    <span className="text-muted-foreground text-sm shrink-0" aria-hidden="true">₹</span>
                    <Input
                      id={`cat-${cat.name}`}
                      type="number"
                      min="0"
                      value={cat.budget === 0 ? '' : cat.budget}
                      placeholder="No limit"
                      onChange={(e) => handleCategoryBudgetChange(cat.name, e.target.value)}
                      aria-label={`${cat.name} monthly budget in Indian rupees`}
                    />
                  </div>
                  {cat.budget > 0 && (
                    <p className="text-xs text-muted-foreground">
                      ₹ {cat.budget.toLocaleString()} / month
                    </p>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Info Card */}
      <Card className="border-border/50 bg-secondary/20">
        <CardContent className="pt-5 flex items-start gap-3">
          <AlertTriangle className="w-5 h-5 text-amber-500 shrink-0 mt-0.5" aria-hidden="true" />
          <div className="text-sm text-muted-foreground">
            <p className="font-medium text-foreground mb-1">How alerts work</p>
            <p>
              Dinero calculates your month-to-date (MTD) spend after every new transaction. When you cross a
              configured threshold, you'll receive an in-app notification. Alerts reset at the start of each
              calendar month.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
