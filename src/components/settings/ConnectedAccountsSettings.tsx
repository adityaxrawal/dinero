/**
 * Manages connected Gmail accounts: adding, viewing sync state, and removing.
 *
 * Removal revokes the OAuth grant rather than merely forgetting the account
 * locally, so access genuinely ends rather than remaining available to a stale
 * token.
 */
import { useState } from 'react';
import { Mail, CheckCircle, AlertTriangle, Loader2 } from 'lucide-react';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';
import RevokeGmailButton from './RevokeGmailButton';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** Manages connected Gmail accounts. */
export default function ConnectedAccountsSettings() {
  const { connectedAccounts, refreshConnectedAccounts } = useGlobalState();
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);

  /** Starts the OAuth flow in the system browser. */
  const handleConnectGmail = async () => {
    if (connectedAccounts.length >= 10) return;
    setIsConnecting(true);
    setConnectError(null);
    try {
      await API.auth.startGoogle();
    } catch (err) {
      setConnectError(getErrorMessage(err));
    } finally {
      await refreshConnectedAccounts();
      setIsConnecting(false);
    }
  };

  /** Disconnects an account, revoking its grant at Google. */
  const handleDisconnectGmail = async (accountId: string) => {
    try {
      await API.auth.disconnectGmail(accountId);
      await refreshConnectedAccounts();
    } catch (err) {
      console.error('Failed to disconnect Gmail:', err);
    }
  };

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 mb-4">
        <Mail className="w-5 h-5 text-[#064E3B]" />
        <h3 className="text-xl font-bold text-[#064E3B]">Connected Accounts</h3>
      </div>
      <p className="text-sm text-[#064E3B]/70 mb-5">
        Connect Gmail securely via local OAuth to automate transaction syncing. We only request read
        access, and extraction happens locally. Up to 10 accounts can be connected; connecting a 2nd
        account or beyond requires an active subscription.
      </p>

      {connectedAccounts.length > 0 && (
        <div className="flex flex-col gap-3 mb-4">
          {connectedAccounts.map((account) => {
            const isDegraded = account.account_status?.toLowerCase() === 'degraded';
            return (
              <div
                key={account.account_id}
                className={`flex items-center gap-3 p-4 rounded-xl border ${
                  isDegraded
                    ? 'bg-amber-500/10 border-amber-500/20'
                    : 'bg-[#064E3B]/5 border-[#064E3B]/10'
                }`}
              >
                {isDegraded ? (
                  <AlertTriangle className="w-5 h-5 text-amber-600" />
                ) : (
                  <CheckCircle className="w-5 h-5 text-emerald-600" />
                )}
                <div className="flex-1">
                  <p
                    className={`text-[13px] font-bold ${isDegraded ? 'text-amber-700' : 'text-emerald-700'}`}
                  >
                    {isDegraded ? 'Needs Reconnection' : 'Gmail Connected'}
                  </p>
                  <p className="text-[12px] font-medium text-[#064E3B]/70 mt-0.5">
                    {account.email}
                  </p>
                  {isDegraded && (
                    <p className="text-[12px] font-medium text-[#064E3B]/70 mt-0.5">
                      Syncing has stopped — Gmail access expired or was revoked outside Dinero.
                    </p>
                  )}
                </div>
                {isDegraded && (
                  <button
                    className="px-3 py-1.5 text-[12px] font-semibold rounded-lg bg-amber-500/20 text-amber-800 hover:bg-amber-500/30 transition-colors"
                    onClick={handleConnectGmail}
                    disabled={isConnecting}
                  >
                    {isConnecting ? 'Reconnecting…' : 'Reconnect'}
                  </button>
                )}
                <RevokeGmailButton
                  email={account.email}
                  onRevoke={() => handleDisconnectGmail(account.account_id)}
                />
              </div>
            );
          })}
        </div>
      )}

      {connectError && (
        <div className="p-4 rounded-xl bg-amber-500/10 border border-amber-500/20 mb-4 text-[13px] font-medium text-amber-700">
          {connectError}
        </div>
      )}

      {connectedAccounts.length < 10 && (
        <button
          className="h-9 px-4 rounded-lg font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 transition-colors inline-flex items-center justify-center disabled:opacity-50"
          onClick={handleConnectGmail}
          disabled={isConnecting}
          id="connect-gmail-btn"
        >
          {isConnecting ? (
            <span className="flex items-center gap-2">
              <Loader2 className="w-4 h-4 animate-spin" /> Connecting…
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
