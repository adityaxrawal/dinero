import { useState, useEffect, useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { Search, Filter, Download, Save, Plus, X, Mail, FileText, PenLine, HelpCircle, Trash2 } from 'lucide-react';
import { API, TransactionRecord, InstrumentRecord } from '../lib/ipc';
import { Card, CardHeader, CardTitle, CardDescription, CardFooter } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';

const JsonViewer = ({ data }: { data: any }) => {
  if (typeof data === 'string') return <span className="text-green-400 break-all">"{data}"</span>;
  if (typeof data === 'number') return <span className="text-orange-400">{data}</span>;
  if (typeof data === 'boolean') return <span className="text-purple-400">{data ? 'true' : 'false'}</span>;
  if (data === null) return <span className="text-muted-foreground">null</span>;

  if (Array.isArray(data)) {
    if (data.length === 0) return <span className="text-muted-foreground">[]</span>;
    return (
      <div className="pl-2 border-l border-border/40 ml-2">
        <span className="text-muted-foreground font-bold">[</span>
        <div className="pl-4 flex flex-col my-1">
          {data.map((item, index) => (
            <div key={index} className="flex">
              <JsonViewer data={item} />
              {index < data.length - 1 && <span className="text-muted-foreground">,</span>}
            </div>
          ))}
        </div>
        <span className="text-muted-foreground font-bold">]</span>
      </div>
    );
  }

  const entries = Object.entries(data);
  if (entries.length === 0) return <span className="text-muted-foreground">{'{'} {'}'}</span>;

  return (
    <div className="pl-2 border-l border-border/40 ml-2">
      <span className="text-muted-foreground font-bold">{'{'}</span>
      <div className="pl-4 flex flex-col my-1">
        {entries.map(([key, value], index) => (
          <div key={key} className="flex flex-wrap items-start">
            <span className="text-blue-400 font-medium mr-2">"{key}":</span>
            <JsonViewer data={value} />
            {index < entries.length - 1 && <span className="text-muted-foreground">,</span>}
          </div>
        ))}
      </div>
      <span className="text-muted-foreground font-bold">{'}'}</span>
    </div>
  );
};

const formatCustomDate = (dateString: string) => {
  const d = new Date(dateString);
  const day = d.getDate();
  const getOrdinal = (n: number) => {
    const s = ["th", "st", "nd", "rd"];
    const v = n % 100;
    return n + (s[(v - 20) % 10] || s[v] || s[0]);
  };
  const month = d.toLocaleString('en-US', { month: 'short' });
  const year = d.getFullYear().toString().slice(-2);
  return `${getOrdinal(day)} ${month} ${year}'`;
};

// G11 fix: source-pipeline icon so a list row shows where a transaction came
// from — previously no row indicated its ingestion source at all.
function SourcePipelineIcon({ sourceMix }: { sourceMix: string | null }) {
  const value = (sourceMix || '').toLowerCase();
  if (value.includes('statement')) {
    return <FileText className="w-3.5 h-3.5 text-muted-foreground" aria-label="From statement" />;
  }
  if (value.includes('manual')) {
    return <PenLine className="w-3.5 h-3.5 text-muted-foreground" aria-label="Manually entered" />;
  }
  if (value.includes('email') || value.includes('gmail')) {
    return <Mail className="w-3.5 h-3.5 text-muted-foreground" aria-label="From email" />;
  }
  return <HelpCircle className="w-3.5 h-3.5 text-muted-foreground" aria-label="Source unknown" />;
}

function evidenceDescription(sourceMix: string | null): { label: string; detail: string } {
  const value = (sourceMix || '').toLowerCase();
  if (value.includes('merged')) {
    return { label: 'Merged Sources', detail: 'Reconciled from multiple matching observations' };
  }
  if (value.includes('statement')) {
    return { label: 'Statement Extraction', detail: 'Parsed from an uploaded/emailed statement' };
  }
  if (value.includes('manual')) {
    return { label: 'Manual Entry', detail: 'Entered directly by you' };
  }
  if (value.includes('email') || value.includes('gmail')) {
    return { label: 'Email Extraction', detail: 'Parsed from a Gmail transaction alert' };
  }
  return { label: 'Unknown Source', detail: sourceMix || 'No source information recorded' };
}

export default function Transactions() {
  const { toast } = useToast();
  const [searchParams] = useSearchParams();
  const initialSearch = searchParams.get('search') || '';
  const [selectedTxn, setSelectedTxn] = useState<TransactionRecord | null>(null);
  const [transactions, setTransactions] = useState<TransactionRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState(initialSearch);
  // G9 fix: real total from the backend, not a hardcoded page count.
  const [currentPage, setCurrentPage] = useState(1);
  const [totalTransactions, setTotalTransactions] = useState(0);
  const PAGE_SIZE = 50;
  const totalPages = Math.max(1, Math.ceil(totalTransactions / PAGE_SIZE));
  const isSearching = searchQuery.trim() !== '';

  // G10 fix: a real (if simple) category filter over the current page,
  // replacing the Filter button that previously had no handler at all.
  const [categoryFilter, setCategoryFilter] = useState<string>('all');

  // Correction Form State
  const [editMerchant, setEditMerchant] = useState('');
  const [editCategory, setEditCategory] = useState('');
  const [editTags, setEditTags] = useState<string[]>([]);
  const [newTag, setNewTag] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  // G13 fix: the real reusable-tag catalog, for autocomplete.
  const [availableTags, setAvailableTags] = useState<string[]>([]);

  useEffect(() => {
    API.tags.list().then(setAvailableTags).catch((err) => console.error('Failed to fetch tags:', err));
  }, []);

  // G12 fix: manual transaction creation UI/IPC — the backend command
  // existed, but there was no ipc.ts wrapper or UI to reach it at all.
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
  const [instruments, setInstruments] = useState<InstrumentRecord[]>([]);
  const [newTxnAmount, setNewTxnAmount] = useState('');
  const [newTxnDirection, setNewTxnDirection] = useState<'debit' | 'credit'>('debit');
  const [newTxnMerchant, setNewTxnMerchant] = useState('');
  const [newTxnDate, setNewTxnDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [newTxnInstrumentId, setNewTxnInstrumentId] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);

  useEffect(() => {
    API.instruments.list().then(setInstruments).catch((err) => console.error('Failed to fetch instruments:', err));
  }, []);

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
      fetchTransactions(searchQuery, currentPage);
    } catch (e: any) {
      toast({ variant: 'destructive', title: 'Create Failed', description: e?.message || String(e) });
    } finally {
      setIsCreating(false);
    }
  };

  const handleDeleteTransaction = async () => {
    if (!selectedTxn) return;
    let confirmed = false;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask('Delete this transaction? This cannot be undone.', { title: 'Delete Transaction', kind: 'warning' });
    } catch {
      confirmed = confirm('Delete this transaction? This cannot be undone.');
    }
    if (!confirmed) return;

    setIsDeleting(true);
    try {
      await API.transactions.delete(selectedTxn.id);
      toast({ title: 'Transaction Deleted' });
      setSelectedTxn(null);
      fetchTransactions(searchQuery, currentPage);
    } catch (e: any) {
      toast({
        variant: 'destructive',
        title: 'Delete Failed',
        description: e?.message || 'Only manually-entered transactions can be deleted.',
      });
    } finally {
      setIsDeleting(false);
    }
  };

  // Source Dialog State
  const [sourceData, setSourceData] = useState<any>(null);
  const [isSourceDialogOpen, setIsSourceDialogOpen] = useState(false);
  const [isSourceLoading, setIsSourceLoading] = useState(false);

  const handleViewSource = async (tx: TransactionRecord) => {
    setIsSourceDialogOpen(true);
    setIsSourceLoading(true);
    try {
      const sourceLog = await API.transactions.getSourceLog(tx.id);
      if (sourceLog) {
        setSourceData(sourceLog);
      } else {
        setSourceData({ error: 'No source data found for this transaction.' });
      }
    } catch (e) {
      console.error(e);
      setSourceData({ error: 'Failed to load source data.' });
    } finally {
      setIsSourceLoading(false);
    }
  };

  const fetchTransactions = useCallback(async (query: string = '', page: number = 1) => {
    setLoading(true);
    try {
      if (query.trim() === '') {
        const { records, total } = await API.transactions.list(page);
        setTransactions(records);
        setTotalTransactions(total);
      } else {
        const txs = await API.transactions.search(query);
        setTransactions(txs);
        setTotalTransactions(txs.length);
      }
    } catch (err) {
      console.error('Failed to fetch transactions:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchTransactions(initialSearch, currentPage);

    const unlistenTx = listen('transaction_created', () => {
      fetchTransactions(initialSearch, currentPage);
    });
    const unlistenStmt = listen('statement_parsed', () => {
      fetchTransactions(initialSearch, currentPage);
    });
    const unlistenScan = listen('scan_completed', () => {
      fetchTransactions(initialSearch, currentPage);
    });

    return () => {
      unlistenTx.then(f => f());
      unlistenStmt.then(f => f());
      unlistenScan.then(f => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetchTransactions, initialSearch, currentPage]);

  const handleSearch = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      setCurrentPage(1);
      setLoading(true);
      try {
        if (searchQuery.trim() === '') {
          await fetchTransactions('', 1);
        } else {
          const txs = await API.transactions.search(searchQuery);
          setTransactions(txs);
          setTotalTransactions(txs.length);
        }
      } catch (err) {
        console.error('Search failed', err);
      } finally {
        setLoading(false);
      }
    }
  };

  const selectTransaction = async (tx: TransactionRecord) => {
    setSelectedTxn(tx);
    setEditMerchant(tx.merchant);
    setEditCategory(tx.category);
    // G13 fix: tags live relationally now — fetch this transaction's real
    // associations rather than reading a `tags` field the backend never sends.
    try {
      const tags = await API.transactions.getTags(tx.id);
      setEditTags(tags);
    } catch (err) {
      console.error('Failed to fetch transaction tags:', err);
      setEditTags([]);
    }
  };

  const handleAddTag = () => {
    if (newTag && !editTags.includes(newTag)) {
      setEditTags([...editTags, newTag]);
      setNewTag('');
    }
  };

  const handleRemoveTag = (tagToRemove: string) => {
    setEditTags(editTags.filter(t => t !== tagToRemove));
  };

  const handleSaveCorrection = async () => {
    if (!selectedTxn) return;
    setIsSaving(true);
    try {
      await API.transactions.update(selectedTxn.id, {
        merchantDisplayName: editMerchant,
        tags: editTags,
      });
      toast({
        title: "Transaction Updated",
        description: "Your corrections have been saved and logged.",
      });
      // Refresh local state (category is not persisted — see Category field note)
      setTransactions(prev => prev.map(t => t.id === selectedTxn.id ? { ...t, merchant: editMerchant, tags: editTags } : t));
      setSelectedTxn({ ...selectedTxn, merchant: editMerchant, tags: editTags });
      if (!availableTags.includes(newTag)) {
        API.tags.list().then(setAvailableTags).catch(() => {});
      }
    } catch (e) {
      console.error(e);
      toast({
        variant: "destructive",
        title: "Update Failed",
        description: "Update failed",
      });
    } finally {
      setIsSaving(false);
    }
  };

  const handleExportCsv = () => {
    // G10 fix: Export previously had no handler at all.
    const header = ['Date', 'Merchant', 'Category', 'Amount', 'Status'];
    const rows = filteredTransactions.map((t) => [
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
    link.download = `transactions-page-${currentPage}.csv`;
    link.click();
    URL.revokeObjectURL(url);
  };

  // G10 fix: Filter previously had no handler at all — filters the current
  // page/search results by category (scoped to what's loaded, not the full
  // dataset, since there's no server-side category filter endpoint yet).
  const categoryOptions = Array.from(new Set(transactions.map((t) => t.category))).sort();
  const filteredTransactions =
    categoryFilter === 'all' ? transactions : transactions.filter((t) => t.category === categoryFilter);

  return (
    <div className="flex flex-row-reverse gap-6 h-[calc(100vh-80px)] animate-in fade-in duration-500 justify-end">
      {/* Detail Drawer (Right Sidebar) moved to top of DOM for E2E tests */}
      {selectedTxn && (
        <Card role="dialog" aria-modal="false" className="w-[380px] flex flex-col shrink-0 animate-in slide-in-from-right-4 shadow-xl border-border/60">
          <CardHeader className="flex flex-row items-start justify-between pb-4 border-b border-border/40">
            <div className="min-w-0">
              <CardTitle>Details</CardTitle>
              <CardDescription className="truncate" title={`Transaction ID: ${selectedTxn.id.split('_')[1] || selectedTxn.id}`}>
                Transaction ID: {selectedTxn.id.split('_')[1] || selectedTxn.id}
              </CardDescription>
            </div>
            <Button variant="ghost" size="icon" className="-mt-2 -mr-2 shrink-0" onClick={() => setSelectedTxn(null)} aria-label="Close" data-testid="detail-close-btn">
              <X className="w-4 h-4" />
            </Button>
          </CardHeader>

          <ScrollArea className="flex-1 p-6">
            <div className="text-center mb-8">
              <h3 className={cn("text-3xl font-bold mb-2", selectedTxn.amount < 0 ? "text-red-700" : "text-emerald-700")}>
                {selectedTxn.amount < 0 ? '- ' : '+ '}₹{Math.abs(selectedTxn.amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
              </h3>
              <p className="text-lg font-medium">{selectedTxn.merchant}</p>
              <p className="text-sm text-muted-foreground">{formatCustomDate(selectedTxn.date)} {new Date(selectedTxn.date).toLocaleTimeString()}</p>
            </div>

            <div className="space-y-6">
              {/* Correction Form */}
              <div className="space-y-4">
                <h4 className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Correction</h4>
                
                <div className="space-y-2">
                  <Label htmlFor="merchant-name">Merchant Name</Label>
                  <Input id="merchant-name" value={editMerchant} onChange={(e) => setEditMerchant(e.target.value)} />
                </div>

                <div className="space-y-2">
                  <Label>Category</Label>
                  {/* Category correction isn't wired to a persistence path yet
                      (no backend contract for it) — shown read-only rather
                      than as a control that silently discards edits. */}
                  <Badge variant="outline" className="font-normal">{editCategory}</Badge>
                </div>

                <div className="space-y-2">
                  <Label>Tags</Label>
                  <div className="flex flex-wrap gap-2 mb-2">
                    {editTags.map(tag => (
                      <Badge key={tag} variant="secondary" className="badge flex items-center gap-1">
                        {tag}
                        <div
                          role="button"
                          aria-label={`Remove tag ${tag}`}
                          className="hover:bg-accent hover:text-accent-foreground w-3 h-3 flex items-center justify-center rounded-sm cursor-pointer"
                          onClick={() => handleRemoveTag(tag)}
                        >
                          <X className="w-3 h-3" aria-hidden="true" />
                        </div>
                      </Badge>
                    ))}
                  </div>
                  <div className="flex gap-2">
                    {/* G13 fix: native autocomplete against the real, reusable
                        tag catalog — previously pure free-text with nothing
                        to autocomplete against. */}
                    <Input
                      aria-label="New tag"
                      placeholder="New tag..."
                      list="tag-suggestions"
                      value={newTag}
                      onChange={(e) => setNewTag(e.target.value)}
                      onKeyDown={(e) => e.key === 'Enter' && handleAddTag()}
                    />
                    <datalist id="tag-suggestions">
                      {availableTags.filter((t) => !editTags.includes(t)).map((t) => (
                        <option key={t} value={t} />
                      ))}
                    </datalist>
                    <Button variant="outline" size="icon" onClick={handleAddTag} aria-label="Add tag"><Plus className="w-4 h-4" aria-hidden="true" /></Button>
                  </div>
                </div>

                <Button className="w-full mt-2" onClick={handleSaveCorrection} disabled={isSaving}>
                  {isSaving ? 'Saving...' : <><Save className="w-4 h-4 mr-2" /> Save Corrections</>}
                </Button>
                {/* G12 fix: no delete path existed in the UI at all (backend
                    already restricts this to manually-entered transactions). */}
                <Button
                  variant="outline"
                  className="w-full text-red-700 hover:text-red-700"
                  onClick={handleDeleteTransaction}
                  disabled={isDeleting}
                >
                  <Trash2 className="w-4 h-4 mr-2" /> {isDeleting ? 'Deleting...' : 'Delete Transaction'}
                </Button>
              </div>

              {/* Evidence Lineage — G10 fix: reflects this transaction's
                  actual source_mix rather than a static hardcoded panel. */}
              <div className="space-y-3 pt-4 border-t border-border/40">
                <h4 className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Evidence Lineage</h4>
                <div className="p-3 bg-secondary/50 rounded-md border-l-2 border-primary text-sm">
                  <p className="font-medium flex items-center gap-2">
                    <SourcePipelineIcon sourceMix={selectedTxn.source_mix} />
                    {evidenceDescription(selectedTxn.source_mix).label}
                  </p>
                  <p className="text-xs text-muted-foreground mt-1">{evidenceDescription(selectedTxn.source_mix).detail}</p>
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-3 w-full"
                    onClick={() => handleViewSource(selectedTxn)}
                  >
                    View Source
                  </Button>
                </div>
              </div>
            </div>
          </ScrollArea>
        </Card>
      )}

      {/* Main Table Area */}
      <div className="flex-1 flex flex-col min-w-0">
        <header className="mb-6 flex flex-wrap justify-between items-end gap-4">
          <div>
            <h1 className="text-3xl font-bold tracking-tight">Transactions</h1>
            <p className="text-muted-foreground mt-1">Canonical records of your spending.</p>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                type="text"
                placeholder="Search... (Press Enter)"
                className="pl-9 w-[250px] bg-card"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={handleSearch}
              />
            </div>
            {/* G10 fix: Filter now actually filters (by category, over the
                currently-loaded rows) instead of being a dead button. */}
            <Select value={categoryFilter} onValueChange={setCategoryFilter}>
              <SelectTrigger className="w-[160px]" aria-label="Filter by category">
                <Filter className="h-4 w-4 mr-1" aria-hidden="true" />
                <SelectValue placeholder="All categories" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All categories</SelectItem>
                {categoryOptions.map((c) => (
                  <SelectItem key={c} value={c}>{c}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="default" onClick={handleExportCsv}>
              <Download className="h-4 w-4 mr-2" /> Export
            </Button>
            {/* G12 fix: manual transaction creation had no UI at all. */}
            <Button variant="default" onClick={() => setIsCreateModalOpen(true)}>
              <Plus className="h-4 w-4 mr-2" /> New Transaction
            </Button>
          </div>
        </header>

        <Card className="flex-1 overflow-hidden flex flex-col border-border/60">
          <ScrollArea className="flex-1">
            <Table>
              <TableHeader className="sticky top-0 bg-card z-10">
                <TableRow>
                  <TableHead className="w-[28px]"></TableHead>
                  <TableHead>Date</TableHead>
                  <TableHead>Merchant</TableHead>
                  <TableHead>Category</TableHead>
                  <TableHead className="text-right">Amount</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="w-[50px]"></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {loading ? (
                  <TableRow>
                    <TableCell colSpan={7} className="text-center h-24">Loading transactions...</TableCell>
                  </TableRow>
                ) : filteredTransactions.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7} className="text-center h-24 text-muted-foreground">No transactions found.</TableCell>
                  </TableRow>
                ) : (
                  filteredTransactions.map((tx) => (
                    <TableRow
                      key={tx.id}
                      onClick={() => selectTransaction(tx)}
                      className={cn("cursor-pointer", selectedTxn?.id === tx.id && "bg-muted/50")}
                    >
                      <TableCell>
                        <SourcePipelineIcon sourceMix={tx.source_mix} />
                      </TableCell>
                      <TableCell className="text-muted-foreground">
                        <div className="flex flex-col">
                          <span>{formatCustomDate(tx.date)}</span>
                          <span className="text-xs">{new Date(tx.date).toLocaleTimeString()}</span>
                        </div>
                      </TableCell>
                      <TableCell className="font-medium">{tx.merchant}</TableCell>
                      <TableCell>
                        <Badge variant="outline" className="font-normal">{tx.category}</Badge>
                      </TableCell>
                      <TableCell className={cn("text-right font-medium", tx.amount < 0 ? "text-red-700" : "text-emerald-700")}>
                        {tx.amount < 0 ? '- ' : '+ '}₹{Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
                      </TableCell>
                      <TableCell>
                        <Badge variant={tx.status.toLowerCase() === 'posted' ? 'default' : 'secondary'} className="text-[10px] px-1.5 py-0.5">
                          {tx.status}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleViewSource(tx);
                          }}
                        >
                          Source
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </ScrollArea>
          <CardFooter className="flex items-center justify-between p-4 border-t border-border/60">
            <span className="text-sm text-muted-foreground">
              {isSearching
                ? `${filteredTransactions.length} search result${filteredTransactions.length === 1 ? '' : 's'}`
                : `Showing page ${currentPage} of ${totalPages} (${totalTransactions} total)`}
            </span>
            <div className="space-x-2">
              <Button variant="outline" size="sm" disabled={isSearching || currentPage === 1} onClick={() => setCurrentPage(p => Math.max(1, p - 1))}>Previous</Button>
              <Button variant="outline" size="sm" disabled={isSearching || currentPage === totalPages} onClick={() => setCurrentPage(p => Math.min(totalPages, p + 1))}>Next</Button>
            </div>
          </CardFooter>
        </Card>
      </div>
      
      <Dialog open={isSourceDialogOpen} onOpenChange={setIsSourceDialogOpen}>
        <DialogContent className="max-w-3xl max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>Transaction Source Data</DialogTitle>
            <DialogDescription>Exact email parsed for this transaction.</DialogDescription>
          </DialogHeader>
          <ScrollArea className="flex-1 mt-4 p-4 bg-black/40 rounded-md font-mono text-sm">
            {isSourceLoading ? (
              'Loading...'
            ) : sourceData ? (
              <div className="py-2">
                {typeof sourceData === 'string' ? (
                  <pre className="whitespace-pre-wrap">{sourceData}</pre>
                ) : (
                  <JsonViewer data={sourceData} />
                )}
              </div>
            ) : (
              'No data'
            )}
          </ScrollArea>
        </DialogContent>
      </Dialog>

      {/* New Transaction modal (G12 fix) */}
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
            <Button
              onClick={handleCreateTransaction}
              disabled={isCreating || !newTxnMerchant.trim() || !newTxnAmount || !newTxnInstrumentId}
            >
              {isCreating ? 'Creating...' : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
