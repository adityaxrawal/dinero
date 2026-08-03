/**
 * Shared helper for native Tauri confirm dialogs with a web-API fallback.
 *
 * Several call sites (transaction/instrument deletes, licence deactivation,
 * revealing the recovery phrase, cluster resolution) each hand-rolled the
 * same try/catch: dynamic-import `ask`, fall back to `window.confirm` when
 * the plugin is unavailable (a bare browser, or vitest/jsdom). Centralised
 * here so every confirm flow picks up both paths for free.
 */
export async function confirmAction(message: string, title: string): Promise<boolean> {
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    return await ask(message, { title, kind: 'warning' });
  } catch {
    return confirm(message);
  }
}

export async function confirmDeleteTransaction(): Promise<boolean> {
  return confirmAction('Delete this transaction? This cannot be undone.', 'Delete Transaction');
}
