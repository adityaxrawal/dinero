import { useState } from 'react';

/**
 * TASK-FE-014 (Doc 30): "per-account revoke with a confirmation dialog
 * explaining new transactions will stop syncing." The pre-existing
 * `handleDisconnectGmail` in Settings.tsx called `auth_google_disconnect`
 * with zero confirmation of any kind -- a single misclick silently cut off
 * a connected account. Reuses this codebase's established
 * `@tauri-apps/plugin-dialog` `ask()` confirmation pattern.
 */
export default function RevokeGmailButton({
  email,
  onRevoke,
}: {
  email: string;
  onRevoke: () => Promise<void>;
}) {
  const [isRevoking, setIsRevoking] = useState(false);

  const handleClick = async () => {
    let confirmed: boolean;
    const warning = `Disconnect ${email}? New transactions from this account will stop syncing immediately. Transactions already imported are not affected or deleted.`;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Disconnect Gmail Account', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setIsRevoking(true);
    try {
      await onRevoke();
    } finally {
      setIsRevoking(false);
    }
  };

  return (
    <button
      className="px-3 py-1.5 text-[12px] font-semibold rounded-lg bg-red-500/10 text-red-700 border border-red-500/20 hover:bg-red-500/20 transition-colors disabled:opacity-50"
      onClick={handleClick}
      disabled={isRevoking}
    >
      {isRevoking ? 'Disconnecting…' : 'Disconnect'}
    </button>
  );
}
