import { useCallback, useState } from 'react'
import { invokeCommand } from '@/lib/ipc'
import type { AppError } from '@/types/ipc'

interface UseIpcInvokeResult<TArgs, TReturn> {
  invoke: (args?: TArgs) => Promise<TReturn>
  loading: boolean
  error: AppError | null
}

/**
 * TASK-SETUP-013. Thin, typed wrapper around a single Tauri command,
 * exposing React-friendly loading/error state.
 *
 * Existing pages call `API.*` (`src/lib/ipc.ts`) directly and manage their
 * own `loading`/`error` state by hand — this hook is additive, for new
 * component code that wants that bookkeeping without repeating it; it is
 * not a required migration for existing call sites.
 */
export function useIpcInvoke<
  TArgs = Record<string, unknown>,
  TReturn = unknown,
>(command: string): UseIpcInvokeResult<TArgs, TReturn> {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<AppError | null>(null)

  const invoke = useCallback(
    async (args?: TArgs): Promise<TReturn> => {
      setLoading(true)
      setError(null)
      try {
        return await invokeCommand<TReturn>(
          command,
          args as Record<string, unknown> | undefined
        )
      } catch (err) {
        setError(err as AppError)
        throw err
      } finally {
        setLoading(false)
      }
    },
    [command]
  )

  return { invoke, loading, error }
}
