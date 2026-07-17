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
      className="btn btn-secondary"
      onClick={handleClick}
      disabled={isRevoking}
      style={{
        padding: '6px 12px',
        fontSize: '12px',
        background: 'rgba(239,68,68,0.1)',
        color: 'var(--error)',
        border: '1px solid rgba(239,68,68,0.2)',
      }}
    >
      {isRevoking ? 'Disconnecting…' : 'Disconnect'}
    </button>
  );
}
