import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Download, Plus, Loader2, Search, X, SlidersHorizontal } from 'lucide-react';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import type { TransactionListFilters } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import { DatePicker } from '@/components/ui/date-picker';
import { cn, formatRelativeDate } from '@/lib/utils';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useTransactionSearch } from '@/hooks/queries/useTransactionSearch';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useQueryClient } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';
import TransactionInspector from '@/components/transactions/TransactionInspector';

const ALL = '__all__';

export default function Transactions() {
  const { toast } = useToast();
  // const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [searchParams] = useSearchParams();

  // Initialise filters from URL params (deep-link support from dashboard categories / instrument detail)
  const [filters, setFilters] = useState<TransactionListFilters>(() => {
    const category = searchParams.get('category');
    const instrument = searchParams.get('instrument');
    return {
      ...(category ? { category_id: category } : {}),
      ...(instrument ? { instrument_id: instrument } : {}),
    };
  });
  const [searchQuery, setSearchQuery] = useState('');
  const isSearching = searchQuery.trim().length > 0;

  // Selected transaction for inspector panel
  const [selectedTxId, setSelectedTxId] = useState<string | null>(null);

  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const infinite = useTransactionsInfiniteList(filters);
  const search = useTransactionSearch(searchQuery);

  const listedTransactions = useMemo(
    () => infinite.data?.pages.flatMap((p) => p.records) ?? [],
    [infinite.data]
  );
  const transactions = useMemo(
    () => (isSearching ? (search.data ?? []) : listedTransactions),
    [isSearching, search.data, listedTransactions]
  );
  const loading = isSearching ? search.isLoading : infinite.isLoading;
  const total = isSearching ? transactions.length : (infinite.data?.pages[0]?.total ?? 0);

  const instrumentById = useMemo(() => new Map(instruments.map((i) => [i.id, i])), [instruments]);

  // Infinite scroll sentinel
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = infinite;
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (isSearching || !hasNextPage) return;
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetchingNextPage) fetchNextPage();
      },
      { rootMargin: '200px' }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [isSearching, hasNextPage, isFetchingNextPage, fetchNextPage]);

  // ── Create Transaction modal ──────────────────────────────
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
  const [newTxnAmount, setNewTxnAmount] = useState('');
  const [newTxnDirection, setNewTxnDirection] = useState<'debit' | 'credit'>('debit');
  const [newTxnMerchant, setNewTxnMerchant] = useState('');
  const [newTxnDate, setNewTxnDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [newTxnInstrumentId, setNewTxnInstrumentId] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const handleCreateTransaction = async () => {
    const amountValue = parseFloat(newTxnAmount);
    if (isNaN(amountValue) || amountValue <= 0 || !newTxnMerchant.trim() || !newTxnInstrumentId)
      return;
    setIsCreating(true);
    try {
      await API.transactions.create({
        amountMinor: Math.round(amountValue * 100),
        currency: 'INR',
        direction: newTxnDirection,
        eventTime: `${newTxnDate} 00:00:00`,
        merchantName: newTxnMerchant.trim(),
        instrumentId: newTxnInstrumentId,
      });
      toast({ title: 'Transaction Created', description: 'Your manual entry has been added.' });
      setIsCreateModalOpen(false);
      setNewTxnAmount('');
      setNewTxnMerchant('');
      setNewTxnDirection('debit');
      setNewTxnInstrumentId('');
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
    } catch (e) {
      toast({ variant: 'destructive', ...getErrorToast(e) });
    } finally {
      setIsCreating(false);
    }
  };

  const handleExportCsv = useCallback(() => {
    const header = ['Date', 'Merchant', 'Category', 'Amount', 'Status'];
    const rows = transactions.map((t) => [
      t.date,
      t.merchant,
      t.category,
      t.amount.toFixed(2),
      t.status,
    ]);
    const csv = [header, ...rows]
      .map((row) => row.map((cell) => `"${String(cell).replace(/"/g, '""')}"`).join(','))
      .join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'transactions-export.csv';
    link.click();
    URL.revokeObjectURL(url);
  }, [transactions]);

  const activeFilterCount = Object.values(filters).filter(Boolean).length;

  const setFilter = <K extends keyof TransactionListFilters>(
    key: K,
    value: TransactionListFilters[K] | undefined
  ) => {
    setFilters((prev) => {
      const next = { ...prev };
      if (value === undefined || value === ALL) {
        delete next[key];
      } else {
        next[key] = value;
      }
      return next;
    });
  };

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Master List (Feed) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20 bg-[#F8E7C9]"
        style={{ width: '320px' }}
      >
        {/* Header bar */}
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">
                All Transactions
              </h1>
              <span
                className="text-[10px] font-medium px-1.5 py-0.5 rounded-md"
                style={{ background: 'rgba(6,78,59,0.07)', color: '#064E3B' }}
              >
                {total.toLocaleString()}
              </span>
            </div>

            <div className="flex items-center gap-1">
              <button
                type="button"
                className="flex items-center justify-center w-7 h-7 rounded-md transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]"
                onClick={handleExportCsv}
                aria-label="Export CSV"
              >
                <Download className="w-3.5 h-3.5" aria-hidden="true" />
              </button>
              <button
                type="button"
                className="flex items-center justify-center w-7 h-7 rounded-md transition-colors bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9]"
                onClick={() => setIsCreateModalOpen(true)}
                aria-label="New transaction"
              >
                <Plus className="w-4 h-4" aria-hidden="true" />
              </button>
            </div>
          </div>

          {/* Search */}
          <div className="relative w-full">
            <Search
              className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 pointer-events-none opacity-50"
              style={{ color: '#064E3B' }}
              aria-hidden="true"
            />
            <input
              type="text"
              placeholder="Search..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setSearchQuery('')}
              className="w-full pl-8 pr-8 h-7 rounded-md text-[13px] outline-none placeholder:text-[#064E3B]/40 focus:ring-1 ring-[#064E3B]/30"
              style={{
                backgroundColor: 'rgba(6,78,59,0.04)',
                border: '1px solid rgba(6,78,59,0.08)',
                color: '#064E3B',
              }}
              aria-label="Search transactions"
            />
            {searchQuery && (
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 opacity-50 hover:opacity-100"
                onClick={() => setSearchQuery('')}
                aria-label="Clear search"
              >
                <X className="w-3.5 h-3.5 text-[#064E3B]" />
              </button>
            )}
          </div>
        </div>

        {/* Filter chips row */}
        {!isSearching && (
          <div className="flex items-center gap-2 px-3 py-2 flex-shrink-0 border-b border-[#064E3B]/10">
            <SlidersHorizontal
              className="w-3 h-3 flex-shrink-0 opacity-40 mx-1"
              style={{ color: '#064E3B' }}
              aria-hidden="true"
            />

            <Select
              value={filters.instrument_id ?? ALL}
              onValueChange={(val) => setFilter('instrument_id', val === ALL ? undefined : val)}
            >
              <SelectTrigger
                className={cn(
                  'h-6 text-[11px] font-medium border-0 rounded-full px-2.5 min-w-[90px] max-w-[120px]',
                  filters.instrument_id
                    ? 'bg-[#064E3B] text-[#F8E7C9]'
                    : 'bg-[#064E3B]/5 text-[#064E3B] hover:bg-[#064E3B]/10'
                )}
              >
                <SelectValue placeholder="Accounts" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All Accounts</SelectItem>
                {instruments.map((inst) => (
                  <SelectItem key={inst.id} value={inst.id}>
                    {inst.issuer_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Select
              value={filters.category_id ?? ALL}
              onValueChange={(val) => setFilter('category_id', val === ALL ? undefined : val)}
            >
              <SelectTrigger
                className={cn(
                  'h-6 text-[11px] font-medium border-0 rounded-full px-2.5 min-w-[90px] max-w-[120px]',
                  filters.category_id
                    ? 'bg-[#064E3B] text-[#F8E7C9]'
                    : 'bg-[#064E3B]/5 text-[#064E3B] hover:bg-[#064E3B]/10'
                )}
              >
                <SelectValue placeholder="Categories" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All Categories</SelectItem>
                {categories.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {activeFilterCount > 0 && (
              <button
                type="button"
                className="filter-chip text-[11px] py-0.5 px-2 rounded-full border text-[#ef4444] border-[#ef4444]/20 hover:bg-[#ef4444]/10"
                onClick={() => setFilters({})}
                aria-label="Clear all filters"
              >
                Clear
              </button>
            )}
          </div>
        )}

        {/* List items */}
        <div className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading...</span>
            </div>
          ) : transactions.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-40">
              <p className="text-xs text-[#064E3B]/50">
                {isSearching ? `No results for "${searchQuery}"` : 'No transactions found.'}
              </p>
            </div>
          ) : (
            <div className="flex flex-col py-1">
              {transactions.map((tx) => {
                const instrument = tx.instrument_id
                  ? instrumentById.get(tx.instrument_id)
                  : undefined;
                const dateStr = formatRelativeDate(tx.date);

                const isSelected = selectedTxId === tx.id;

                return (
                  <button
                    key={tx.id}
                    className={cn(
                      'flex flex-col w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none',
                      isSelected
                        ? 'bg-[#064E3B] text-[#F8E7C9]'
                        : 'hover:bg-[#064E3B]/5 text-[#064E3B]'
                    )}
                    onClick={() => setSelectedTxId(tx.id)}
                  >
                    <div className="flex items-start justify-between w-full mb-1">
                      <span
                        className={cn(
                          'font-semibold text-[13px] truncate pr-2',
                          isSelected ? 'text-white' : 'text-[#064E3B]'
                        )}
                      >
                        {tx.merchant}
                      </span>
                      <span
                        className={cn(
                          'text-[13px] font-semibold whitespace-nowrap',
                          isSelected
                            ? 'text-white'
                            : tx.direction === 'debit'
                              ? 'text-red-700'
                              : 'text-[#10b981]'
                        )}
                      >
                        {tx.direction === 'debit' ? '−' : '+'}₹
                        {Math.abs(tx.amount).toLocaleString(undefined, {
                          minimumFractionDigits: 0,
                        })}
                      </span>
                    </div>

                    <div className="flex items-center justify-between w-full text-[11px] opacity-70 font-medium">
                      <span className="truncate pr-2">
                        {tx.category} • {instrument ? instrument.issuer_name : 'Unknown'}
                      </span>
                      <span className="whitespace-nowrap flex-shrink-0">{dateStr}</span>
                    </div>
                  </button>
                );
              })}

              {!isSearching && hasNextPage && (
                <div ref={sentinelRef} className="flex justify-center py-4">
                  <button
                    type="button"
                    className="text-xs font-medium px-4 py-1.5 rounded-full border border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                    onClick={() => fetchNextPage()}
                    disabled={isFetchingNextPage}
                  >
                    {isFetchingNextPage ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 animate-spin inline mr-1.5" />
                        Loading…
                      </>
                    ) : (
                      'Load more'
                    )}
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ──────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col">
        {selectedTxId ? (
          <TransactionInspector
            transactionId={selectedTxId}
            onClose={() => setSelectedTxId(null)}
            categories={categories}
          />
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
            <div className="w-12 h-12 border-2 border-[#064E3B] rounded-xl mb-4 border-dashed flex items-center justify-center">
              <span className="text-[#064E3B] font-bold text-xl">D</span>
            </div>
            <p className="text-[#064E3B] font-medium text-sm">
              Select a transaction to view details
            </p>
          </div>
        )}
      </div>

      {/* ── Create Transaction Modal ─────────────────────────── */}
      <Dialog open={isCreateModalOpen} onOpenChange={setIsCreateModalOpen}>
        <DialogContent className="sm:max-w-[425px]">
          <DialogHeader>
            <DialogTitle>New Transaction</DialogTitle>
            <DialogDescription>
              Manually record a transaction not captured automatically.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-2">
              <Label htmlFor="new-txn-merchant">Merchant</Label>
              <Input
                id="new-txn-merchant"
                value={newTxnMerchant}
                onChange={(e) => setNewTxnMerchant(e.target.value)}
                placeholder="e.g. Amazon"
              />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label htmlFor="new-txn-amount">Amount (₹)</Label>
                <Input
                  id="new-txn-amount"
                  type="number"
                  min="0"
                  step="0.01"
                  value={newTxnAmount}
                  onChange={(e) => setNewTxnAmount(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label>Direction</Label>
                <Select
                  value={newTxnDirection}
                  onValueChange={(v) => setNewTxnDirection(v as 'debit' | 'credit')}
                >
                  <SelectTrigger aria-label="Direction">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="debit">Debit (spend)</SelectItem>
                    <SelectItem value="credit">Credit (income)</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="new-txn-date">Date</Label>
              <DatePicker
                id="new-txn-date"
                value={newTxnDate}
                onChange={(val) => setNewTxnDate(val)}
              />
            </div>
            <div className="space-y-2">
              <Label>Instrument</Label>
              <Select value={newTxnInstrumentId} onValueChange={setNewTxnInstrumentId}>
                <SelectTrigger aria-label="Instrument">
                  <SelectValue placeholder="Select instrument" />
                </SelectTrigger>
                <SelectContent>
                  {instruments.map((inst) => (
                    <SelectItem key={inst.id} value={inst.id}>
                      {inst.issuer_name} •••• {inst.masked_identifier}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setIsCreateModalOpen(false)}>
              Cancel
            </Button>
            <Button
              onClick={handleCreateTransaction}
              disabled={
                isCreating || !newTxnMerchant.trim() || !newTxnAmount || !newTxnInstrumentId
              }
            >
              {isCreating ? 'Creating...' : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
