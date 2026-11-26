/**
 * Exports the ledger to a CSV file the user picks.
 *
 * The backend writes every row matching the current filters (or search), with
 * a direction column and formula-safe text. The old export ran here over only
 * the pages already loaded and had no direction (audit F11-1, F11-2, F11-3).
 */
import { API, type TransactionListFilters } from '@/lib/ipc';

/** Asks where to save, then exports. Resolves to the row count, or null if cancelled. */
export async function exportTransactionsCsv(
  filters: TransactionListFilters,
  searchQuery: string
): Promise<number | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: `dinero-transactions-${new Date().toISOString().slice(0, 10)}.csv`,
    filters: [{ name: 'CSV', extensions: ['csv'] }],
  });
  if (!path) return null;
  return API.transactions.exportCsv(path, filters, searchQuery);
}
