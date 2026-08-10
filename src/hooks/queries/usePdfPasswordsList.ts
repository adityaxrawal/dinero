import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Saved statement-PDF passwords, listed for management in settings.
 *
 * Returns metadata about which instruments have a stored password, not the
 * secrets themselves; those stay in the OS keychain on the Rust side.
 */
export function usePdfPasswordsList() {
  return useQuery({
    queryKey: queryKeys.pdfPasswords.list(),
    queryFn: API.pdfPasswords.list,
  });
}
