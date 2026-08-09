import { formatRelativeDate } from '@/lib/utils';

export interface DateGroup<T> {
  dateLabel: string;
  items: T[];
}

/** Consecutive runs sharing a relative date label ("Today", "Yesterday", …).
 *  Runs, not a keyed bucket: the list arrives already sorted, and a same-label
 *  run appearing twice would mean the sort broke, which should stay visible. */
export function groupByDateLabel<T extends { date: string }>(items: T[]): DateGroup<T>[] {
  const groups: DateGroup<T>[] = [];
  let currentLabel = '';
  let currentItems: T[] = [];

  for (const item of items) {
    const dateLabel = formatRelativeDate(item.date);
    if (dateLabel !== currentLabel) {
      if (currentItems.length > 0) groups.push({ dateLabel: currentLabel, items: currentItems });
      currentLabel = dateLabel;
      currentItems = [item];
    } else {
      currentItems.push(item);
    }
  }
  if (currentItems.length > 0) groups.push({ dateLabel: currentLabel, items: currentItems });

  return groups;
}
