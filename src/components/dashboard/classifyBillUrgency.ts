/**
 * Colour-codes an upcoming bill by days until due.
 *
 * Status colours are reserved roles and are never reused for arbitrary series.
 */
export type BillUrgency = 'overdue' | 'critical' | 'warning' | 'normal';

/**
 * Colour-codes a bill by days until due.
 *
 * Status colours are reserved roles and never reused for arbitrary series.
 */
export function classifyBillUrgency(dueDate: string, now: Date = new Date()): BillUrgency {
  const due = new Date(dueDate);
  const msPerDay = 24 * 60 * 60 * 1000;
  const daysUntilDue = Math.ceil((due.getTime() - now.getTime()) / msPerDay);

  if (daysUntilDue < 0) return 'overdue';
  if (daysUntilDue <= 3) return 'critical';
  if (daysUntilDue <= 7) return 'warning';
  return 'normal';
}
