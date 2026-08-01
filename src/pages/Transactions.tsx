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
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  // Selected transaction for inspector panel
  const [selectedTxId, setSelectedTxId] = useState<string | null>(null);

  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const infinite = useTransactionsInfiniteList(filters);
  const search = useTransactionSearch(searchQuery, filters);

  // Global shortcut to focus search input (Cmd+K / Ctrl+K)
  useEffect(() => {
    const handleCmdK = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', handleCmdK);
    return () => window.removeEventListener('keydown', handleCmdK);
  }, []);

  const categoryNameById = useMemo(
    () => new Map(categories.map((c) => [c.id, c.name])),
    [categories]
  );

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
  };  // Keyboard navigation through list items
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable)
      ) {
        return;
      }
      if (!transactions || transactions.length === 0) return;

      const currentIndex = selectedTxId
        ? transactions.findIndex((t) => t.id === selectedTxId)
        : -1;

      if (e.key === 'ArrowDown' || e.key === 'j') {
        e.preventDefault();
        const nextIndex = Math.min(transactions.length - 1, currentIndex + 1);
        if (transactions[nextIndex]) setSelectedTxId(transactions[nextIndex].id);
      } else if (e.key === 'ArrowUp' || e.key === 'k') {
        e.preventDefault();
        const prevIndex = Math.max(0, currentIndex - 1);
        if (transactions[prevIndex]) setSelectedTxId(transactions[prevIndex].id);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [transactions, selectedTxId]);

  const groupedTransactions = useMemo(() => {
    const groups: { dateLabel: string; items: typeof transactions }[] = [];
    let currentLabel = '';
    let currentItems: typeof transactions = [];

    for (const tx of transactions) {
      const dateLabel = formatRelativeDate(tx.date);
      if (dateLabel !== currentLabel) {
        if (currentItems.length > 0) {
          groups.push({ dateLabel: currentLabel, items: currentItems });
        }
        currentLabel = dateLabel;
        currentItems = [tx];
      } else {
        currentItems.push(tx);
      }
    }
    if (currentItems.length > 0) {
      groups.push({ dateLabel: currentLabel, items: currentItems });
    }
    return groups;
  }, [transactions]);

  return (
    <div className="flex h-full w-full overflow-hidden select-none">
      {/* ── Column 2: Master List (Feed) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/15 bg-[#F8E7C9]"
        style={{ width: '340px' }}
      >
        {/* Header bar */}
        <div className="flex flex-col gap-3 px-4 py-3.5 flex-shrink-0 border-b border-[#064E3B]/10 bg-[#F8E7C9]/60 backdrop-blur-sm">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h1 className="text-[15px] font-bold text-[#064E3B] tracking-tight">
                All Transactions
              </h1>
              <span
                className="text-[11px] font-bold px-2 py-0.5 rounded-full font-mono shadow-2xs"
                style={{ background: 'rgba(6,78,59,0.08)', color: '#064E3B' }}
              >
                {total.toLocaleString()}
              </span>
            </div>

            <div className="flex items-center gap-1.5">
              <button
                type="button"
                className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B] cursor-pointer"
                onClick={handleExportCsv}
                aria-label="Export CSV"
                title="Export CSV"
              >
                <Download className="w-4 h-4" aria-hidden="true" />
              </button>
              <button
                type="button"
                className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-2xs cursor-pointer"
                onClick={() => setIsCreateModalOpen(true)}
                aria-label="New transaction"
                title="Record transaction"
              >
                <Plus className="w-4 h-4" aria-hidden="true" />
              </button>
            </div>
          </div>

          {/* Search */}
          <div className="relative w-full">
            <Search
              className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 pointer-events-none text-[#064E3B]/50"
              aria-hidden="true"
            />
            <input
              ref={searchInputRef}
              type="text"
              placeholder="Search merchant, category, amount, account..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setSearchQuery('')}
              className="w-full pl-8 pr-12 h-8 rounded-xl text-[12px] font-medium outline-none placeholder:text-[#064E3B]/40 focus:ring-1 focus:ring-[#064E3B]/30 focus:border-[#064E3B]/40 transition-all bg-[#F3EBDD]/60 border border-[#064E3B]/15 text-[#064E3B]"
              aria-label="Search transactions"
            />
            {searchQuery ? (
              <button
                type="button"
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
                onClick={() => setSearchQuery('')}
                aria-label="Clear search"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            ) : (
              <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] font-mono font-medium text-[#064E3B]/40 bg-[#064E3B]/5 px-1.5 py-0.5 rounded border border-[#064E3B]/10 pointer-events-none">
                ⌘K
              </kbd>
            )}
          </div>
        </div>

        {/* Filter chips row */}
        <div className="flex items-center gap-1.5 px-3.5 py-2 flex-shrink-0 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.02]">
          <SlidersHorizontal
            className="w-3 h-3 flex-shrink-0 opacity-40 mx-0.5 text-[#064E3B]"
            aria-hidden="true"
          />

          <Select
            value={filters.instrument_id ?? ALL}
            onValueChange={(val) => setFilter('instrument_id', val === ALL ? undefined : val)}
          >
            <SelectTrigger
              className={cn(
                'h-6 text-[11px] font-semibold border-0 rounded-full px-2.5 min-w-[85px] max-w-[125px] cursor-pointer shadow-2xs',
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
                'h-6 text-[11px] font-semibold border-0 rounded-full px-2.5 min-w-[85px] max-w-[125px] cursor-pointer shadow-2xs',
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
              className="filter-chip text-[10px] font-bold py-0.5 px-2 rounded-full border text-red-600 border-red-500/20 hover:bg-red-500/10 cursor-pointer ml-auto"
              onClick={() => setFilters({})}
              aria-label="Clear all filters"
            >
              Reset
            </button>
          )}
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto px-2 py-2 space-y-3">
          {loading ? (
            <div className="flex flex-col items-center justify-center h-48 gap-2">
              <Loader2 className="w-5 h-5 animate-spin text-[#064E3B]/60" />
              <span className="text-xs font-medium text-[#064E3B]/60">Loading transactions…</span>
            </div>
          ) : transactions.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 px-4 text-center">
              <p className="text-xs font-medium text-[#064E3B]/60">
                {isSearching ? `No transactions match "${searchQuery}"` : 'No transactions found.'}
              </p>
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {groupedTransactions.map((group) => (
                <div key={group.dateLabel} className="space-y-1">
                  <div className="sticky top-0 z-10 px-2 py-1 bg-[#F8E7C9]/90 backdrop-blur-xs text-[10px] font-bold text-[#064E3B]/60 uppercase tracking-wider">
                    {group.dateLabel}
                  </div>
                  {group.items.map((tx) => {
                    const instrument = tx.instrument_id
                      ? instrumentById.get(tx.instrument_id)
                      : undefined;
                    const isSelected = selectedTxId === tx.id;

                    return (
                      <button
                        key={tx.id}
                        className={cn(
                          'flex items-center gap-3 w-full text-left px-3 py-2.5 rounded-xl transition-all cursor-pointer border select-none',
                          isSelected
                            ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-sm'
                            : 'bg-[#F8E7C9]/40 hover:bg-[#064E3B]/5 border-transparent text-[#064E3B]'
                        )}
                        onClick={() => setSelectedTxId(tx.id)}
                      >
                        {/* Merchant Avatar Icon */}
                        <div
                          className={cn(
                            'w-8 h-8 rounded-lg flex items-center justify-center text-[13px] font-bold shrink-0 transition-colors',
                            isSelected
                              ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
                              : 'bg-[#064E3B]/10 text-[#064E3B]'
                          )}
                        >
                          {tx.merchant?.charAt(0).toUpperCase() ?? '?'}
                        </div>

                        <div className="flex-1 min-w-0">
                          <div className="flex items-center justify-between gap-1 mb-0.5">
                            <span
                              className={cn(
                                'font-semibold text-[13px] truncate pr-1',
                                isSelected ? 'text-white' : 'text-[#064E3B]'
                              )}
                            >
                              {tx.merchant}
                            </span>
                            <span
                              className={cn(
                                'text-[13px] font-bold font-mono whitespace-nowrap shrink-0',
                                isSelected
                                  ? 'text-white'
                                  : tx.direction === 'debit'
                                    ? 'text-red-700'
                                    : 'text-emerald-700'
                              )}
                            >
                              {tx.direction === 'debit' ? '−' : '+'}₹
                              {Math.abs(tx.amount).toLocaleString(undefined, {
                                minimumFractionDigits: 0,
                              })}
                            </span>
                          </div>

                          <div className="flex items-center justify-between text-[11px] font-medium opacity-80 gap-1 mt-0.5">
                            <span className="truncate">
                              {categoryNameById.get(tx.category) || tx.category || 'Uncategorized'}
                              {instrument ? ` • ${instrument.issuer_name}` : ''}
                            </span>
                            <span
                              className={cn(
                                'text-[9px] font-extrabold px-1.5 py-0.2 rounded uppercase tracking-wider border shrink-0',
                                isSelected
                                  ? 'bg-white/20 text-white border-white/30'
                                  : tx.direction === 'debit'
                                    ? 'bg-red-500/10 text-red-700 border-red-500/20'
                                    : 'bg-emerald-500/10 text-emerald-700 border-emerald-500/20'
                              )}
                            >
                              {tx.direction === 'debit' ? 'DEBIT' : 'CREDIT'}
                            </span>
                          </div>
                        </div>
                      </button>
                    );
                  })}
                </div>
              ))}

              {!isSearching && hasNextPage && (
                <div ref={sentinelRef} className="flex justify-center py-3">
                  <button
                    type="button"
                    className="text-xs font-semibold px-4 py-1.5 rounded-full border border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors cursor-pointer"
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
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-40 gap-3">
            <div className="w-14 h-14 border-2 border-[#064E3B] rounded-2xl border-dashed flex items-center justify-center bg-[#064E3B]/5">
              <span className="text-[#064E3B] font-extrabold text-2xl">D</span>
            </div>
            <p className="text-[#064E3B] font-semibold text-sm">
              Select a transaction to inspect details &amp; edit
            </p>
            <p className="text-[#064E3B]/60 text-xs font-mono">
              Use ↑ / ↓ arrow keys to quickly navigate
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
