/** True when running inside the Tauri shell (vs. a bare browser, e.g. Vitest/jsdom or `vite dev` in a tab). */
export function isTauriRuntime(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  );
}
