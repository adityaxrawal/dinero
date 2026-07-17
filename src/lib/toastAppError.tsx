import { ToastAction } from '@/components/ui/toast';
import { toast } from '@/hooks/use-toast';
import { mapAppErrorToToast } from '@/lib/errorMapping';
import type { AppError } from '@/types/ipc';

/**
 * TASK-FE-018: the one function that turns a raw `AppError` into a queued
 * toast via `errorMapping.ts`. Extracted from `ToastProvider.tsx` (which
 * has a component default export) to keep this a plain function module —
 * `react-refresh/only-export-components` flags mixing component and
 * non-component exports in the same file (same pattern fixed in
 * TASK-FE-006's `scanProgressPercent.ts`).
 */
export function toastAppError(error: AppError) {
  const content = mapAppErrorToToast(error);
  const actionTo = content.actionTo;
  toast({
    variant: 'destructive',
    title: content.title,
    description: content.description,
    ...(actionTo
      ? {
          action: (
            <ToastAction
              altText={content.actionLabel ?? 'Open'}
              onClick={() => {
                window.location.hash = actionTo;
              }}
            >
              {content.actionLabel ?? 'Open'}
            </ToastAction>
          ),
        }
      : {}),
  });
}
