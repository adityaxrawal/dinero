import { Toaster } from '@/components/ui/toaster';

/**
 * TASK-FE-018 (Doc 30): "a global toast queue triggered automatically by
 * any failed useIpcInvoke call, so components don't need to manually wire
 * error toasts per mutation."
 *
 * This app's toast primitive (`src/hooks/use-toast.ts`, the standard
 * shadcn pattern) is already a module-level singleton store, not a React
 * Context -- `toast()` is callable from anywhere, including outside a
 * component, with no provider indirection required to reach it. So this
 * component is a thin, honest wrapper rather than inventing a Context this
 * codebase doesn't need: it just renders the actual toast viewport
 * (`<Toaster />`). The dispatcher that turns a raw `AppError` into a
 * queued toast via `errorMapping.ts` is `toastAppError`
 * (`src/lib/toastAppError.tsx`) -- `useIpcInvoke` calls it automatically on
 * every failure; existing React Query mutation hooks across the app
 * already call `toast()` directly in their own `onError` handlers (a
 * correct, working, already-tested pattern) and are not migrated here —
 * this is the shared entry point for new call sites, not a mass refactor
 * of stable existing ones.
 */
export default function ToastProvider() {
  return <Toaster />;
}
