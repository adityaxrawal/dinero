import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API, type RecheckOutcome, type RecheckSummary } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** Toast copy for a single re-check. */
export function recheckMessage(outcome: RecheckOutcome): { title: string; description: string } {
  switch (outcome) {
    case 'booked':
      return {
        title: 'Booked',
        description: 'Your learned patterns completed this transaction.',
      };
    case 'queued_for_ai':
      return {
        title: 'Sent to the AI',
        description: "It's back here only if the AI can't find the details in the email.",
      };
    case 'no_ai_available':
      return {
        title: 'Nothing new found',
        description: 'Learned patterns found nothing more, and no AI model can run on this device.',
      };
    case 'already_queued':
      return { title: 'Already re-checking', description: 'The AI is already reading this email.' };
  }
}

/** Toast copy for "Re-check all". */
export function recheckAllMessage(s: RecheckSummary): { title: string; description: string } {
  const parts = [
    s.booked && `${s.booked} booked`,
    s.queued_for_ai && `${s.queued_for_ai} sent to the AI`,
    s.no_ai_available && `${s.no_ai_available} unchanged (no AI available)`,
    s.skipped && `${s.skipped} skipped`,
  ].filter(Boolean);
  return { title: 'Re-check finished', description: parts.join(', ') || 'Nothing to re-check.' };
}

function invalidate(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.all() });
  queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
  queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
}

/**
 * Reruns learned patterns and the rule layers on one review entry; books it
 * if they complete it, otherwise hands it to the local AI.
 */
export function useRecheckUnassigned() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => API.reconciliation.recheckUnassigned(id),
    onSuccess: () => invalidate(queryClient),
  });
}

/** Re-checks every review entry that keeps its email. */
export function useRecheckAllUnassigned() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => API.reconciliation.recheckAllUnassigned(),
    onSuccess: () => invalidate(queryClient),
  });
}
