import { useState } from 'react';
import { LineChart, Line, XAxis, YAxis, Tooltip, CartesianGrid, ResponsiveContainer } from 'recharts';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { SpendTrendGranularity } from '@/lib/ipc';
import { useSpendTrend } from '@/hooks/queries/useSpendTrend';
import { SEQUENTIAL_LINE_COLOR } from './chartPalette';

const GRANULARITIES: { value: SpendTrendGranularity; label: string }[] = [
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

/**
 * TASK-FE-008 (Doc 30): line chart with a granularity toggle. Single series
 * (total spend over time) — one axis, one sequential hue, no legend needed
 * (the card title already names the series; dataviz skill's legend rule
 * only kicks in at >=2 series).
 */
export default function SpendTrendChart() {
  const [granularity, setGranularity] = useState<SpendTrendGranularity>('daily');
  const { data: trend, isLoading } = useSpendTrend(granularity);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <div>
          <CardTitle>Spend Trend</CardTitle>
          <CardDescription>Confirmed spend over time.</CardDescription>
        </div>
        <div className="flex gap-1" role="group" aria-label="Trend granularity">
          {GRANULARITIES.map((g) => (
            <Button
              key={g.value}
              type="button"
              variant="outline"
              size="sm"
              aria-pressed={granularity === g.value}
              className={cn('h-7 px-2 text-xs', granularity === g.value && 'bg-[#2563eb]/10 border-[#2563eb]/40 text-[#1d4ed8]')}
              onClick={() => setGranularity(g.value)}
            >
              {g.label}
            </Button>
          ))}
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="h-56 flex items-center justify-center text-sm text-muted-foreground" role="status">
            Loading…
          </div>
        ) : !trend || trend.length === 0 ? (
          <div className="h-56 flex items-center justify-center text-sm text-muted-foreground" role="status">
            No spend recorded in this window yet.
          </div>
        ) : (
          <div className="h-56" role="img" aria-label="Line chart of spend over time">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={trend} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" vertical={false} />
                <XAxis dataKey="period" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
                <YAxis tick={{ fontSize: 11 }} className="fill-muted-foreground" width={56} />
                <Tooltip formatter={(value) => `₹ ${Number(value).toLocaleString()}`} />
                <Line
                  type="monotone"
                  dataKey="total_spend"
                  stroke={SEQUENTIAL_LINE_COLOR}
                  strokeWidth={2}
                  dot={{ r: 3 }}
                  activeDot={{ r: 5 }}
                />
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
