/**
 * Revokes Gmail access for a connected account.
 *
 * Calls Google's revocation endpoint rather than just deleting the local token,
 * so the grant is withdrawn at the source.
 */
import { useState } from 'react';

/** Revokes Gmail access for an account. */
export default function RevokeGmailButton({
  email,
  onRevoke,
}: {
  email: string;
  onRevoke: () => Promise<void>;
}) {
  const [isRevoking, setIsRevoking] = useState(false);

  /** Confirms, then revokes the grant at Google. */
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
