/**
 * Mounts the toast viewport.
 *
 * A thin wrapper by design -- the toast store is a module-level singleton, so no
 * React Context is needed and inventing one would add indirection for nothing.
 */
import { Toaster } from '@/components/ui/toaster';

/** Mounts the toast viewport; a thin wrapper since the store is a singleton. */
export default function ToastProvider() {
  return <Toaster />;
}
