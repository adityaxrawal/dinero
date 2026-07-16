import { useEffect, useState } from 'react';
import { Search } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';

interface TransactionSearchBoxProps {
  onQueryChange: (query: string) => void;
}

const DEBOUNCE_MS = 300;

/** TASK-FE-009 (Doc 30): 300ms-debounced FTS5-backed search box. */
export default function TransactionSearchBox({ onQueryChange }: TransactionSearchBoxProps) {
  const [raw, setRaw] = useState('');
  const debounced = useDebouncedValue(raw, DEBOUNCE_MS);

  useEffect(() => {
    onQueryChange(debounced.trim());
    // onQueryChange is expected to be referentially stable (a useCallback at
    // the call site) -- omitted from deps so an inline arrow prop doesn't
    // re-fire this effect on every parent render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [debounced]);

  return (
    <div className="relative">
      <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" aria-hidden="true" />
      <Input
        type="text"
        placeholder="Search transactions…"
        className="pl-9 w-[250px] bg-card"
        value={raw}
        onChange={(e) => setRaw(e.target.value)}
        aria-label="Search transactions"
      />
    </div>
  );
}
