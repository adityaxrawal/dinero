interface ExportRow {
  date: string;
  merchant: string;
  category: string;
  amount: number;
  status: string;
}

const HEADER = ['Date', 'Merchant', 'Category', 'Amount', 'Status'];

const quote = (cell: unknown) => `"${String(cell).replace(/"/g, '""')}"`;

function toCsv(transactions: ExportRow[]): string {
  const rows = transactions.map((t) => [t.date, t.merchant, t.category, t.amount.toFixed(2), t.status]);
  return [HEADER, ...rows].map((row) => row.map(quote).join(',')).join('\n');
}

/** Downloads the current list as a CSV via a transient object URL. */
export function downloadTransactionsCsv(transactions: ExportRow[]): void {
  const blob = new Blob([toCsv(transactions)], { type: 'text/csv;charset=utf-8;' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'transactions-export.csv';
  link.click();
  URL.revokeObjectURL(url);
}
