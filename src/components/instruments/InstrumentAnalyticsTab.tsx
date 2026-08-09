import { useMemo } from 'react';
import {PieChart, ShoppingBag, ArrowUpRight, ArrowDownRight} from 'lucide-react';
import type { TransactionRecord } from '@/lib/ipc';

interface InstrumentAnalyticsTabProps {
  transactions: TransactionRecord[];
}

export default function InstrumentAnalyticsTab({ transactions }: InstrumentAnalyticsTabProps) {
  const stats = useMemo(() => {
    let inflow = 0;
    let outflow = 0;
    const categoryMap: Record<string, number> = {};

    transactions.forEach((tx) => {
      const isDebit = tx.direction === 'debit' || tx.amount < 0;
      if (isDebit) {
        outflow += Math.abs(tx.amount);
      } else {
        inflow += Math.abs(tx.amount);
      }

      const cat = tx.category || tx.merchant || 'Uncategorized';
      categoryMap[cat] = (categoryMap[cat] || 0) + Math.abs(tx.amount);
    });

    const topCategories = Object.entries(categoryMap)
      .map(([name, amount]) => ({ name, amount }))
      .sort((a, b) => b.amount - a.amount)
      .slice(0, 5);

    const avgTx = transactions.length > 0 ? (inflow + outflow) / transactions.length : 0;

    return {
      inflow,
      outflow,
      totalCount: transactions.length,
      topCategories,
      avgTx,
    };
  }, [transactions]);

  if (transactions.length === 0) {
    return (
      <div className="text-center py-12 bg-[#F8E7C9]/40 rounded-2xl border border-[#064E3B]/10 p-6">
        <PieChart className="w-8 h-8 text-[#064E3B]/40 mx-auto mb-2" />
        <h4 className="text-sm font-bold text-[#064E3B]">No Analytics Data Available</h4>
        <p className="text-xs text-[#064E3B]/60 mt-1">
          Upload statements or record transactions for this account to see spending insights.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-4 animate-in fade-in-50 duration-200">
      {/* 2x2 Summary Metric Cards */}
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-[#F8E7C9]/70 border border-[#064E3B]/10 rounded-2xl p-3.5 space-y-1">
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">Total Outflow</span>
            <div className="w-6 h-6 rounded-full bg-red-500/15 text-red-700 flex items-center justify-center">
              <ArrowUpRight className="w-3.5 h-3.5" />
            </div>
          </div>
          <p className="text-xl font-extrabold font-mono text-red-700">
            ₹{stats.outflow.toLocaleString(undefined, { minimumFractionDigits: 2 })}
          </p>
        </div>

        <div className="bg-[#F8E7C9]/70 border border-[#064E3B]/10 rounded-2xl p-3.5 space-y-1">
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">Total Inflow</span>
            <div className="w-6 h-6 rounded-full bg-emerald-500/15 text-emerald-700 flex items-center justify-center">
              <ArrowDownRight className="w-3.5 h-3.5" />
            </div>
          </div>
          <p className="text-xl font-extrabold font-mono text-emerald-700">
            ₹{stats.inflow.toLocaleString(undefined, { minimumFractionDigits: 2 })}
          </p>
        </div>
      </div>

      {/* Metric Details */}
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-2xl p-3">
          <span className="text-[10px] font-bold uppercase tracking-wider text-[#064E3B]/60 block">Total Activity</span>
          <span className="text-sm font-extrabold font-mono text-[#064E3B]">{stats.totalCount} transactions</span>
        </div>
        <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-2xl p-3">
          <span className="text-[10px] font-bold uppercase tracking-wider text-[#064E3B]/60 block">Avg Transaction</span>
          <span className="text-sm font-extrabold font-mono text-[#064E3B]">
            ₹{stats.avgTx.toLocaleString(undefined, { maximumFractionDigits: 2 })}
          </span>
        </div>
      </div>

      {/* Top Categories / Merchants Breakdown */}
      <div className="bg-[#F8E7C9]/60 border border-[#064E3B]/10 rounded-2xl p-4 space-y-3">
        <div className="flex items-center justify-between border-b border-[#064E3B]/10 pb-2">
          <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]">Top Volume Items</h4>
          <ShoppingBag className="w-4 h-4 text-[#064E3B]/60" />
        </div>

        <div className="space-y-2">
          {stats.topCategories.map((item, idx) => {
            const totalVolume = stats.inflow + stats.outflow;
            const pct = totalVolume > 0 ? (item.amount / totalVolume) * 100 : 0;
            return (
              <div key={idx} className="space-y-1">
                <div className="flex justify-between items-center text-xs font-semibold text-[#064E3B]">
                  <span className="truncate max-w-[180px]">{item.name}</span>
                  <span className="font-mono font-bold">₹{item.amount.toLocaleString()}</span>
                </div>
                <div className="w-full h-1.5 rounded-full bg-[#064E3B]/10 overflow-hidden">
                  <div
                    className="h-full bg-[#064E3B] rounded-full transition-all duration-300"
                    style={{ width: `${Math.min(100, Math.max(3, pct))}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
