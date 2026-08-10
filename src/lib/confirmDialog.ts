/**
 * Confirmation prompts for destructive actions, backed by a native dialog.
 *
 * Prefers the operating system's own dialog through the Tauri plugin so the
 * prompt looks and behaves like the rest of the desktop environment, and falls
 * back to the browser's built-in `confirm` when that plugin is unreachable --
 * which is the case in a plain browser during development and under test.
 */

/**
 * Ask the user to confirm an action, resolving true only if they accept.
 *
 * The plugin is imported dynamically rather than at module load so that merely
 * importing this file does not pull the Tauri dialog code into environments
 * that have no Tauri runtime to service it.
 */
export async function confirmAction(message: string, title: string): Promise<boolean> {
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    return await ask(message, { title, kind: 'warning' });
  } catch {
    // No native dialog available (browser or test environment) -- degrade to
    // the DOM prompt rather than failing the action outright.
    return confirm(message);
  }
}

/**
 * Standard confirmation for deleting a transaction.
 *
 * Wrapped as its own function so the wording stays identical everywhere the
 * deletion can be triggered from.
 */
export async function confirmDeleteTransaction(): Promise<boolean> {
  return confirmAction('Delete this transaction? This cannot be undone.', 'Delete Transaction');
}
