/**
 * Exports the current transaction view to CSV.
 *
 * Fields are quoted and escaped, since merchant names routinely contain commas
 * and quotes that would otherwise corrupt the file.
 */
interface ExportRow {
  date: string;
  merchant: string;
  category: string;
  amount: number;
  status: string;
}

const HEADER = ['Date', 'Merchant', 'Category', 'Amount', 'Status'];

/**
 * Quotes a CSV cell, doubling any embedded quotes.
 *
 * Required because merchant names routinely contain commas and quotes, which
 * would otherwise split a row into the wrong number of columns.
 */
const quote = (cell: unknown) => `"${String(cell).replace(/"/g, '""')}"`;

/** Builds the CSV text, header first. */
function toCsv(transactions: ExportRow[]): string {
  const rows = transactions.map((t) => [t.date, t.merchant, t.category, t.amount.toFixed(2), t.status]);
  return [HEADER, ...rows].map((row) => row.map(quote).join(',')).join('\n');
}

/**
 * Triggers a CSV download in the browser.
 *
 * The object URL is revoked immediately after the click, since it would
 * otherwise pin the blob in memory for the page's lifetime.
 */
export function downloadTransactionsCsv(transactions: ExportRow[]): void {
  const blob = new Blob([toCsv(transactions)], { type: 'text/csv;charset=utf-8;' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'transactions-export.csv';
  link.click();
  URL.revokeObjectURL(url);
}
