/**
 * Shared helper for native Tauri confirm dialogs with a web-API fallback.
 *
 * `TransactionQuickActions` and `TransactionDetail` previously both
 * hand-rolled the exact same try/catch pattern (import ask → fallback to
 * window.confirm). Centralised here so any future delete flow picks up
 * both the Tauri plugin and the fallback for free.
 */
export async function confirmDeleteTransaction(): Promise<boolean> {
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    return await ask('Delete this transaction? This cannot be undone.', {
      title: 'Delete Transaction',
      kind: 'warning',
    });
  } catch {
    return confirm('Delete this transaction? This cannot be undone.');
  }
}
