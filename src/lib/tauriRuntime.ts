/**
 * Detects whether the frontend is running inside the Tauri desktop shell.
 *
 * The same bundle is loaded in three places -- the packaged desktop window, a
 * plain browser during `vite dev`, and jsdom under Vitest -- but only the first
 * has a Rust backend to talk to. Call sites use this to fall back to browser
 * behaviour instead of invoking IPC commands that would throw.
 *
 * Detection works by probing for the internals object Tauri injects into
 * `window` at startup, guarded by a `typeof window` check so the function is
 * also safe to call where no DOM exists at all.
 */
export function isTauriRuntime(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  );
}
