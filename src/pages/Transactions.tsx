import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Download, Plus, Loader2 } from 'lucide-react';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/getErrorMessage';
import type { TransactionListFilters } from '@/lib/ipc';
import { Card, CardFooter } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useTransactionSearch } from '@/hooks/queries/useTransactionSearch';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useQueryClient } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';
import TransactionFilterBar from '@/components/transactions/TransactionFilterBar';
import TransactionSearchBox from '@/components/transactions/TransactionSearchBox';
import TransactionRow from '@/components/transactions/TransactionRow';

export default function Transactions() {
  const { toast } = useToast();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [searchParams] = useSearchParams();

  // TASK-FE-008's CategoryBreakdownChart navigates here with ?category=<id>.
  const [filters, setFilters] = useState<TransactionListFilters>(() => {
    const category = searchParams.get('category');
    return category ? { category_id: category } : {};
  });
  const [searchQuery, setSearchQuery] = useState('');
  const isSearching = searchQuery.trim().length > 0;

  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const infinite = useTransactionsInfiniteList(filters);
  const search = useTransactionSearch(searchQuery);

  const listedTransactions = useMemo(
    () => infinite.data?.pages.flatMap((p) => p.records) ?? [],
    [infinite.data],
  );
  const transactions = useMemo(
    () => (isSearching ? search.data ?? [] : listedTransactions),
    [isSearching, search.data, listedTransactions],
  );
  const loading = isSearching ? search.isLoading : infinite.isLoading;
  const total = isSearching ? transactions.length : infinite.data?.pages[0]?.total ?? 0;

  const instrumentById = useMemo(() => new Map(instruments.map((i) => [i.id, i])), [instruments]);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  // TASK-FE-009: infinite-scroll via a bottom sentinel + IntersectionObserver,
  // with a visible "Load more" button as a reliable fallback (scroll-physics
  // in headless/e2e environments can be flaky).
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = infinite;
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (isSearching || !hasNextPage) return;
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetchingNextPage) {
          fetchNextPage();
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [isSearching, hasNextPage, isFetchingNextPage, fetchNextPage]);

  // Create Transaction modal
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
  const [newTxnAmount, setNewTxnAmount] = useState('');
  const [newTxnDirection, setNewTxnDirection] = useState<'debit' | 'credit'>('debit');
  const [newTxnMerchant, setNewTxnMerchant] = useState('');
  const [newTxnDate, setNewTxnDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [newTxnInstrumentId, setNewTxnInstrumentId] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const handleCreateTransaction = async () => {
    const amountValue = parseFloat(newTxnAmount);
    if (isNaN(amountValue) || amountValue <= 0 || !newTxnMerchant.trim() || !newTxnInstrumentId) return;
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
      toast({ variant: 'destructive', title: 'Create Failed', description: getErrorMessage(e) });
    } finally {
      setIsCreating(false);
    }
  };

  const handleExportCsv = useCallback(() => {
    const header = ['Date', 'Merchant', 'Category', 'Amount', 'Status'];
    const rows = transactions.map((t) => [t.date, t.merchant, t.category, t.amount.toFixed(2), t.status]);
    const csv = [header, ...rows].map((row) => row.map((cell) => `"${String(cell).replace(/"/g, '""')}"`).join(',')).join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'transactions-export.csv';
    link.click();
    URL.revokeObjectURL(url);
  }, [transactions]);

  return (
    <div className="flex flex-col gap-6 h-[calc(100vh-80px)] animate-in fade-in duration-500">
      <header className="flex flex-wrap justify-between items-end gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Transactions</h1>
          <p className="text-muted-foreground mt-1">Canonical records of your spending.</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <TransactionSearchBox onQueryChange={setSearchQuery} />
          <Button variant="default" onClick={handleExportCsv}>
            <Download className="h-4 w-4 mr-2" /> Export
          </Button>
          <Button variant="default" onClick={() => setIsCreateModalOpen(true)}>
            <Plus className="h-4 w-4 mr-2" /> New Transaction
          </Button>
        </div>
      </header>

      {!isSearching && <TransactionFilterBar filters={filters} onChange={setFilters} />}

      <Card className="flex-1 overflow-hidden flex flex-col border-border/60">
        <ScrollArea className="flex-1">
          <Table>
            <TableHeader className="sticky top-0 bg-card z-10">
              <TableRow>
                <TableHead className="w-[28px]" />
                <TableHead>Date</TableHead>
                <TableHead>Merchant</TableHead>
                <TableHead>Category</TableHead>
                <TableHead>Instrument</TableHead>
                <TableHead className="text-right">Amount</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="w-[140px]">Quick Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {loading ? (
                <TableRow>
                  <TableCell colSpan={8} className="text-center h-24 text-muted-foreground">Loading transactions…</TableCell>
                </TableRow>
              ) : transactions.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={8} className="text-center h-24 text-muted-foreground">
                    No transactions found.
                  </TableCell>
                </TableRow>
              ) : (
                transactions.map((tx) => (
                  <TransactionRow
                    key={tx.id}
                    tx={tx}
                    instrument={tx.instrument_id ? instrumentById.get(tx.instrument_id) : undefined}
                    category={categoryById.get(tx.category)}
                    categories={categories}
                    isSelected={false}
                    onClick={() => navigate(`/transactions/${tx.id}`)}
                  />
                ))
              )}
            </TableBody>
          </Table>
          {!isSearching && infinite.hasNextPage && (
            <div ref={sentinelRef} className="flex justify-center py-4">
              <Button variant="outline" size="sm" onClick={() => infinite.fetchNextPage()} disabled={infinite.isFetchingNextPage}>
                {infinite.isFetchingNextPage ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : null}
                Load more
              </Button>
            </div>
          )}
        </ScrollArea>
        <CardFooter className="flex items-center justify-between p-4 border-t border-border/60">
          <span className="text-sm text-muted-foreground">
            {isSearching
              ? `${transactions.length} search result${transactions.length === 1 ? '' : 's'}`
              : `${transactions.length} of ${total} loaded`}
          </span>
        </CardFooter>
      </Card>

      <Dialog open={isCreateModalOpen} onOpenChange={setIsCreateModalOpen}>
        <DialogContent className="sm:max-w-[425px]">
          <DialogHeader>
            <DialogTitle>New Transaction</DialogTitle>
            <DialogDescription>Manually record a transaction not captured automatically.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-2">
              <Label htmlFor="new-txn-merchant">Merchant</Label>
              <Input id="new-txn-merchant" value={newTxnMerchant} onChange={(e) => setNewTxnMerchant(e.target.value)} placeholder="e.g. Amazon" />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label htmlFor="new-txn-amount">Amount (₹)</Label>
                <Input id="new-txn-amount" type="number" min="0" step="0.01" value={newTxnAmount} onChange={(e) => setNewTxnAmount(e.target.value)} />
              </div>
              <div className="space-y-2">
                <Label>Direction</Label>
                <Select value={newTxnDirection} onValueChange={(v) => setNewTxnDirection(v as 'debit' | 'credit')}>
                  <SelectTrigger aria-label="Direction"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="debit">Debit (spend)</SelectItem>
                    <SelectItem value="credit">Credit (income)</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="new-txn-date">Date</Label>
              <Input id="new-txn-date" type="date" value={newTxnDate} onChange={(e) => setNewTxnDate(e.target.value)} />
            </div>
            <div className="space-y-2">
              <Label>Instrument</Label>
              <Select value={newTxnInstrumentId} onValueChange={setNewTxnInstrumentId}>
                <SelectTrigger aria-label="Instrument"><SelectValue placeholder="Select instrument" /></SelectTrigger>
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
            <Button variant="outline" onClick={() => setIsCreateModalOpen(false)}>Cancel</Button>
            <Button onClick={handleCreateTransaction} disabled={isCreating || !newTxnMerchant.trim() || !newTxnAmount || !newTxnInstrumentId}>
              {isCreating ? 'Creating...' : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
