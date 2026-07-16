import { Tag } from 'lucide-react';
import { TableCell, TableRow } from '@/components/ui/table';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import type { TransactionRecord, InstrumentRecord, CategoryRecord } from '@/lib/ipc';
import SourcePipelineIcon from './SourcePipelineIcon';
import TransactionQuickActions from './TransactionQuickActions';

interface TransactionRowProps {
  tx: TransactionRecord;
  instrument: InstrumentRecord | undefined;
  category: CategoryRecord | undefined;
  categories: CategoryRecord[];
  isSelected: boolean;
  onClick: () => void;
}

/**
 * TASK-FE-009 (Doc 30): merchant, color-coded amount, category icon, date,
 * instrument badge, source-pipeline icon. `instrument`/`category` are
 * resolved by the parent from its already-loaded instrument/category lists
 * (joining client-side rather than adding a per-row backend fetch).
 */
export default function TransactionRow({ tx, instrument, category, categories, isSelected, onClick }: TransactionRowProps) {
  return (
    <TableRow
      onClick={onClick}
      className={cn('cursor-pointer', isSelected && 'bg-muted/50')}
      aria-selected={isSelected}
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
        <Badge
          variant="outline"
          className="font-normal gap-1"
          style={category?.color ? { borderColor: category.color, color: category.color } : undefined}
        >
          {category?.icon ? <span aria-hidden="true">{category.icon}</span> : <Tag className="w-3 h-3" aria-hidden="true" />}
          {tx.category}
        </Badge>
      </TableCell>
      <TableCell>
        {instrument ? (
          <Badge variant="secondary" className="font-normal text-xs">
            {instrument.issuer_name} •••• {instrument.masked_identifier}
          </Badge>
        ) : (
          <span className="text-xs text-muted-foreground">—</span>
        )}
      </TableCell>
      <TableCell className={cn('text-right font-medium', tx.amount < 0 ? 'text-red-700' : 'text-emerald-700')}>
        {tx.amount < 0 ? '- ' : '+ '}₹{Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
      </TableCell>
      <TableCell>
        <Badge variant={tx.status.toLowerCase() === 'posted' ? 'default' : 'secondary'} className="text-[10px] px-1.5 py-0.5">
          {tx.status}
        </Badge>
      </TableCell>
      <TableCell>
        <TransactionQuickActions transactionId={tx.id} currentCategoryId={tx.category} categories={categories} />
      </TableCell>
    </TableRow>
  );
}
