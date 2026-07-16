import { Filter } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import type { TransactionListFilters } from '@/lib/ipc';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';

interface TransactionFilterBarProps {
  filters: TransactionListFilters;
  onChange: (filters: TransactionListFilters) => void;
}

const ALL = '__all__';

/**
 * TASK-FE-009 (Doc 30): instrument/category/direction/date filters, all
 * AND-combined — matches `transactions_list`'s real filter semantics
 * (Document 19 §8.1: from_date/to_date/instrument_id/direction/category_id/
 * status). Amount-range and tag filters aren't part of that real contract
 * (Doc30's task text names them, but no backend support exists — see
 * fix-log TASK-FE-009), so not built here.
 */
export default function TransactionFilterBar({ filters, onChange }: TransactionFilterBarProps) {
  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const set = <K extends keyof TransactionListFilters>(key: K, value: TransactionListFilters[K] | typeof ALL) => {
    const next = { ...filters };
    if (value === ALL || value === '' || value === undefined) {
      delete next[key];
    } else {
      next[key] = value as TransactionListFilters[K];
    }
    onChange(next);
  };

  const activeCount = Object.values(filters).filter((v) => v !== undefined && v !== '').length;

  return (
    <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Transaction filters">
      <Filter className="h-4 w-4 text-muted-foreground shrink-0" aria-hidden="true" />

      <Select value={filters.instrument_id ?? ALL} onValueChange={(v) => set('instrument_id', v)}>
        <SelectTrigger className="w-[160px]" aria-label="Filter by instrument">
          <SelectValue placeholder="All instruments" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>All instruments</SelectItem>
          {instruments.map((inst) => (
            <SelectItem key={inst.id} value={inst.id}>
              {inst.issuer_name} •••• {inst.masked_identifier}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.category_id ?? ALL} onValueChange={(v) => set('category_id', v)}>
        <SelectTrigger className="w-[150px]" aria-label="Filter by category">
          <SelectValue placeholder="All categories" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>All categories</SelectItem>
          {categories.map((c) => (
            <SelectItem key={c.id} value={c.id}>
              {c.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={filters.direction ?? ALL}
        onValueChange={(v) => set('direction', v === ALL ? ALL : (v as 'debit' | 'credit'))}
      >
        <SelectTrigger className="w-[130px]" aria-label="Filter by direction">
          <SelectValue placeholder="All" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>Debit &amp; Credit</SelectItem>
          <SelectItem value="debit">Debit only</SelectItem>
          <SelectItem value="credit">Credit only</SelectItem>
        </SelectContent>
      </Select>

      <Input
        type="date"
        aria-label="From date"
        className="w-[140px]"
        value={filters.from_date ?? ''}
        onChange={(e) => set('from_date', e.target.value)}
      />
      <Input
        type="date"
        aria-label="To date"
        className="w-[140px]"
        value={filters.to_date ?? ''}
        onChange={(e) => set('to_date', e.target.value)}
      />

      {activeCount > 0 && (
        <Button variant="ghost" size="sm" onClick={() => onChange({})} aria-label="Clear all filters">
          Clear ({activeCount})
        </Button>
      )}
    </div>
  );
}
