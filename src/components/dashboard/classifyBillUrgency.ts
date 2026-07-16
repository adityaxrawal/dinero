export type BillUrgency = 'overdue' | 'critical' | 'warning' | 'normal';

/**
 * TASK-FE-008: color-codes an upcoming bill by days-until-due.
 * Status colors are reserved roles (never reused for arbitrary series) —
 * overdue/critical map to the app's destructive red, warning to amber,
 * normal to a neutral muted tone.
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
