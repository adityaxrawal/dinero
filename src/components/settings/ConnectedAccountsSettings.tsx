import { useState } from 'react';
import { Mail, CheckCircle, AlertTriangle, Loader2 } from 'lucide-react';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/getErrorMessage';
import RevokeGmailButton from './RevokeGmailButton';
import { useGlobalState } from '@/lib/GlobalStateContext';

/**
 * TASK-FE-015 (Doc 30): "lists all connected accounts with per-account
 * status badges, a 'Connect Another Gmail Account' button (re-triggering
 * OAuth for multi-account support), and independent revoke per account."
 *
 * Real backend gap found+fixed: `settings_get_connected_accounts` filtered
 * to `account_status = 'ACTIVE'` only, so a 'degraded' account (Gmail token
 * refresh failed -- TASK-GMAIL's own token-refresh-failure lifecycle event,
 * `gmail_token_refresh_failed`) silently vanished from this list instead of
 * surfacing a status the user could act on. Fixed to include it, and this
 * component is what actually surfaces the badge + a reconnect action.
 */
export default function ConnectedAccountsSettings() {
  const { connectedAccounts, refreshConnectedAccounts } = useGlobalState();
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);

  const handleConnectGmail = async () => {
    if (connectedAccounts.length >= 10) return;
    setIsConnecting(true);
    setConnectError(null);
    try {
      await API.auth.startGoogle();
    } catch (err) {
      // Doc 03 §8.2: connecting a 2nd-10th account requires an ACTIVE
      // subscription — the backend refuses with a specific, user-facing
      // message we surface here rather than a generic OAuth failure.
      setConnectError(getErrorMessage(err));
    } finally {
      // Always refresh — OAuth may have succeeded even if invoke threw a non-critical error
      await refreshConnectedAccounts();
      setIsConnecting(false);
    }
  };

  const handleDisconnectGmail = async (accountId: string) => {
    try {
      await API.auth.disconnectGmail(accountId);
      await refreshConnectedAccounts();
    } catch (err) {
      console.error('Failed to disconnect Gmail:', err);
    }
  };

  return (
    <div className="glass-panel" style={{ padding: '24px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
        <Mail className="text-accent" size={24} color="var(--accent-primary)" />
        <h3 className="heading-md">Connected Accounts</h3>
      </div>
      <p className="text-sm text-muted-foreground" style={{ marginBottom: '20px' }}>
        Connect Gmail securely via local OAuth to automate transaction
        syncing. We only request read access, and extraction happens locally.
        Up to 10 accounts can be connected; connecting a 2nd account or
        beyond requires an active subscription.
      </p>

      {connectedAccounts.length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', marginBottom: '16px' }}>
          {connectedAccounts.map((account) => {
            const isDegraded = account.account_status?.toLowerCase() === 'degraded';
            return (
              <div
                key={account.account_id}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: '12px',
                  padding: '12px 16px',
                  borderRadius: '8px',
                  background: isDegraded ? 'rgba(245,158,11,0.08)' : 'rgba(34,197,94,0.08)',
                  border: `1px solid ${isDegraded ? 'rgba(245,158,11,0.25)' : 'rgba(34,197,94,0.25)'}`,
                }}
              >
                {isDegraded ? <AlertTriangle size={18} color="#b45309" /> : <CheckCircle size={18} color="var(--success)" />}
                <div style={{ flex: 1 }}>
                  <p style={{ fontSize: '13px', fontWeight: 600, color: isDegraded ? '#b45309' : '#047857' }}>
                    {isDegraded ? 'Needs Reconnection' : 'Gmail Connected'}
                  </p>
                  <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '2px' }}>
                    {account.email}
                  </p>
                  {isDegraded && (
                    <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '2px' }}>
                      Syncing has stopped — Gmail access expired or was revoked outside Dinero.
                    </p>
                  )}
                </div>
                {isDegraded && (
                  <button className="btn btn-secondary" onClick={handleConnectGmail} disabled={isConnecting} style={{ padding: '6px 12px', fontSize: '12px' }}>
                    {isConnecting ? 'Reconnecting…' : 'Reconnect'}
                  </button>
                )}
                <RevokeGmailButton email={account.email} onRevoke={() => handleDisconnectGmail(account.account_id)} />
              </div>
            );
          })}
        </div>
      )}

      {connectError && (
        <div
          style={{
            padding: '12px 16px',
            borderRadius: '8px',
            background: 'rgba(245,158,11,0.08)',
            border: '1px solid rgba(245,158,11,0.2)',
            marginBottom: '16px',
            fontSize: '13px',
            color: 'var(--text-muted)',
          }}
        >
          {connectError}
        </div>
      )}

      {connectedAccounts.length < 10 && (
        <button
          className="btn btn-primary"
          onClick={handleConnectGmail}
          disabled={isConnecting}
          id="connect-gmail-btn"
        >
          {isConnecting ? (
            <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <Loader2 size={16} className="animate-spin" /> Connecting…
            </span>
          ) : connectedAccounts.length === 0 ? (
            'Connect Gmail'
          ) : (
            'Connect Another Gmail Account'
          )}
        </button>
      )}
    </div>
  );
}
