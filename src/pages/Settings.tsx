import { Mail, Palette, Cpu, CheckCircle, ScanLine, Loader2, CalendarRange, AlertCircle, FileText, Ban, AlertTriangle, KeyRound, CreditCard, RefreshCw, Wand2, Gauge } from 'lucide-react';
import { API, LlmModelInfo, LicenseStatusResponse, PatternRuleHealth } from '../lib/ipc';
import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import NetworkActivity from '../components/NetworkActivity';
import { Button } from '@/components/ui/button';
import PrivacySettings from '../components/settings/PrivacySettings';
import ConnectedAccountsSettings from '../components/settings/ConnectedAccountsSettings';
import StatementPasswordSettings from '../components/settings/StatementPasswordSettings';

import { useGlobalState } from '../lib/GlobalStateContext';

export default function Settings() {
  const navigate = useNavigate();
  const {
    scanStartDate, setScanStartDate,
    scanEndDate, setScanEndDate,
    scanStatus, setScanStatus,
    scanProgress, setScanProgress,
    scanError, setScanError,
    connectedAccounts,
    handleStartScan
  } = useGlobalState();

  // Doc 16 §12.3: the 5-tier model catalog, fetched from the backend — not
  // hardcoded here, so this can never drift from src-tauri's own list again.
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [model, setModel] = useState(
    localStorage.getItem('llm_model') || 'gemma4_12b',
  );
  useEffect(() => {
    API.llm.getAvailableModels()
      .then(setAvailableModels)
      .catch((err) => console.error('Failed to fetch LLM model catalog:', err));
  }, []);
  const connectedAccount = connectedAccounts[0] ?? null; // primary account, used by Mail Scan below
  const [recoveryPhrase, setRecoveryPhrase] = useState<string | null>(null);
  const [isFetchingPhrase, setIsFetchingPhrase] = useState(false);

  // Doc 22 §8.2: opt-in only, never prompted during onboarding — the consent
  // screen must plainly state that the phrase alone is sufficient to decrypt
  // the user's data on *any* computer, bypassing hardware-bound protection.
  const handleViewRecoveryPhrase = async () => {
    let confirmed = false;
    const warning =
      'Only use this if you anticipate losing both your Mac and your Keychain. ' +
      'Anyone who has this 24-word phrase can decrypt your financial data on any computer — ' +
      'it bypasses this Mac\'s hardware-bound protection entirely. Keep it as secure as your data itself.';
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Secure Backup Recovery Phrase', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setIsFetchingPhrase(true);
    try {
      const phrase = await API.auth.getRecoveryPhrase();
      setRecoveryPhrase(phrase);
    } catch (err: any) {
      console.error('Failed to fetch recovery phrase:', err);
      alert('Failed to retrieve recovery phrase: ' + (err?.message ?? String(err)));
    } finally {
      setIsFetchingPhrase(false);
    }
  };

  // ── License & Billing (C9: wire the existing license_* IPC commands into
  // Settings — they were registered on the backend but unreachable from any UI) ──
  const [licenseStatus, setLicenseStatus] = useState<LicenseStatusResponse | null>(null);
  const [isLoadingLicense, setIsLoadingLicense] = useState(true);
  const [isRefreshingLicense, setIsRefreshingLicense] = useState(false);
  const [isDeactivating, setIsDeactivating] = useState(false);
  const [licenseActionError, setLicenseActionError] = useState<string | null>(null);
  const [showActivateForm, setShowActivateForm] = useState(false);
  const [activateEmail, setActivateEmail] = useState('');
  const [activatePaymentId, setActivatePaymentId] = useState('');
  const [activateSignature, setActivateSignature] = useState('');
  const [activateBillingInterval, setActivateBillingInterval] = useState('monthly');
  const [isActivating, setIsActivating] = useState(false);

  const loadLicenseStatus = async () => {
    setIsLoadingLicense(true);
    try {
      const status = await API.licensing.getStatus();
      setLicenseStatus(status);
    } catch (err) {
      console.error('Failed to fetch license status:', err);
    } finally {
      setIsLoadingLicense(false);
    }
  };

  useEffect(() => {
    loadLicenseStatus();
  }, []);

  const handleRefreshLicense = async () => {
    setIsRefreshingLicense(true);
    setLicenseActionError(null);
    try {
      await API.licensing.refresh();
      await loadLicenseStatus();
    } catch (err: any) {
      setLicenseActionError(err?.message ?? String(err));
    } finally {
      setIsRefreshingLicense(false);
    }
  };

  const handleDeactivateLicense = async () => {
    let confirmed = false;
    const warning = "Deactivate this device's license? Paid features will be unavailable here until you reactivate.";
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Deactivate License', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setIsDeactivating(true);
    setLicenseActionError(null);
    try {
      await API.licensing.deactivate();
      await loadLicenseStatus();
    } catch (err: any) {
      setLicenseActionError(err?.message ?? String(err));
    } finally {
      setIsDeactivating(false);
    }
  };

  const handleActivateLicense = async () => {
    setIsActivating(true);
    setLicenseActionError(null);
    try {
      await API.licensing.activate(
        activateEmail.trim(),
        activatePaymentId.trim(),
        activateSignature.trim(),
        activateBillingInterval,
      );
      setShowActivateForm(false);
      setActivateEmail('');
      setActivatePaymentId('');
      setActivateSignature('');
      await loadLicenseStatus();
    } catch (err: any) {
      setLicenseActionError(err?.message ?? String(err));
    } finally {
      setIsActivating(false);
    }
  };

  // ── Pattern Rules (G14 fix: a real Settings toggle, not Debug-only/read-only) ──
  const [patternRules, setPatternRules] = useState<PatternRuleHealth[]>([]);
  const [isLoadingRules, setIsLoadingRules] = useState(true);
  const [togglingRuleId, setTogglingRuleId] = useState<string | null>(null);

  const loadPatternRules = async () => {
    setIsLoadingRules(true);
    try {
      const rules = await API.patternRules.list();
      setPatternRules(rules);
    } catch (err) {
      console.error('Failed to fetch pattern rules:', err);
    } finally {
      setIsLoadingRules(false);
    }
  };

  useEffect(() => {
    loadPatternRules();
  }, []);

  const handleToggleRule = async (rule: PatternRuleHealth) => {
    setTogglingRuleId(rule.id);
    try {
      await API.patternRules.setStatus(rule.id, rule.is_active ? 'inactive' : 'active');
      await loadPatternRules();
    } catch (err: any) {
      console.error('Failed to update pattern rule:', err);
      alert('Failed to update pattern rule: ' + (err?.message ?? String(err)));
    } finally {
      setTogglingRuleId(null);
    }
  };

  // ── Data Export / Restore from Backup (G4 fix) ───────────────────────
  const [isRestoring, setIsRestoring] = useState(false);

  // J7 fix: a real local encrypted export of the full dataset (transactions,
  // statements, instruments, etc.) — distinct from the diagnostic bundle
  // above, which deliberately excludes financial data.
  const [isExportingData, setIsExportingData] = useState(false);
  const handleExportFullData = async () => {
    setIsExportingData(true);
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const destination = await save({
        defaultPath: `dinero-export-${new Date().toISOString().slice(0, 10)}.db`,
        filters: [{ name: 'Dinero Encrypted Export', extensions: ['db'] }],
      });
      if (!destination) return;
      const path = await API.db.exportData(destination);
      alert('Your encrypted data export is saved to: ' + path + '\n\nThis file is encrypted the same way as your live database and can only be opened by Dinero on this Mac.');
    } catch (err: any) {
      alert('Failed to export data: ' + (err?.message ?? String(err)));
    } finally {
      setIsExportingData(false);
    }
  };

  const handleRestoreFromBackup = async () => {
    let confirmed = false;
    const warning =
      'Restore from the most recent backup? Your current database will be replaced and the app will restart. ' +
      'Any changes made since that backup was taken will be lost.';
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Restore from Backup', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setIsRestoring(true);
    try {
      await API.db.restoreBackup();
      // The backend calls app.restart() on success — if we're still here,
      // something prevented it.
    } catch (err: any) {
      alert('Failed to restore backup: ' + (err?.message ?? String(err)));
      setIsRestoring(false);
    }
  };

  const handleModelChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newModel = e.target.value;
    const selected = availableModels.find((m) => m.id === newModel);
    if (selected) {
      try {
        const ramGb = await API.dev.checkSystemRam();
        const override = localStorage.getItem('llm_ram_override') === 'true';
        if (ramGb < selected.min_ram_gb && !override) {
          if (
            !confirm(
              `Warning: Your system has ${ramGb.toFixed(1)}GB of RAM. ${selected.name} requires at least ${selected.min_ram_gb}GB. Allow override?`,
            )
          ) {
            return;
          }
          localStorage.setItem('llm_ram_override', 'true');
        }
      } catch (err) {
        console.error(err);
      }
    }
    setModel(newModel);
    localStorage.setItem('llm_model', newModel);
  };

  const handleCancelScan = () => {
    setScanStatus('idle');
    setScanProgress(null);
    setScanError(null);
  };

  return (
    <div
      className="animate-fade-in"
      style={{ display: 'flex', flexDirection: 'column', gap: '24px' }}
    >
      <div>
        <h1 className="heading-lg">Settings</h1>
        <p className="text-sm text-muted-foreground" style={{ marginTop: '4px' }}>
          Manage your account, preferences, and local data.
        </p>
      </div>

      {/* ── Connected Accounts (TASK-FE-015) ──────────────────────────── */}
      <ConnectedAccountsSettings />

      {/* ── Mail Scan ──────────────────────────────────────────────────── */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '8px' }}>
          <ScanLine className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Mail Scan</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '20px' }}>
          Scan your Gmail inbox for financial emails within a custom date range.
          The scan runs locally — your emails never leave your device.
        </p>

        {!connectedAccount && (
          <div
            style={{
              padding: '12px 16px',
              borderRadius: '8px',
              background: 'rgba(245,158,11,0.08)',
              border: '1px solid rgba(245,158,11,0.2)',
              marginBottom: '20px',
              fontSize: '13px',
              color: 'var(--text-muted)',
            }}
          >
            ⚠️ Connect a Gmail account above before scanning.
          </div>
        )}

        {/* Date range picker */}
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginBottom: '20px' }}>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
            <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
              <CalendarRange size={12} style={{ display: 'inline', marginRight: '4px' }} />
              Start Date
            </span>
            <input
              id="scan-start-date"
              type="date"
              value={scanStartDate}
              onChange={(e) => setScanStartDate(e.target.value)}
              disabled={scanStatus === 'running' || !connectedAccount}
              className="form-input"
              style={{ padding: '8px 12px', borderRadius: '8px', background: 'var(--bg-secondary)', border: '1px solid var(--border)', color: 'var(--text-primary)', fontSize: '13px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
            <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
              <CalendarRange size={12} style={{ display: 'inline', marginRight: '4px' }} />
              End Date
            </span>
            <input
              id="scan-end-date"
              type="date"
              value={scanEndDate}
              onChange={(e) => setScanEndDate(e.target.value)}
              disabled={scanStatus === 'running' || !connectedAccount}
              className="form-input"
              style={{ padding: '8px 12px', borderRadius: '8px', background: 'var(--bg-secondary)', border: '1px solid var(--border)', color: 'var(--text-primary)', fontSize: '13px' }}
            />
          </label>
        </div>

        {/* Connected account indicator */}
        {connectedAccount && (
          <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '16px' }}>
            Scanning account: <strong style={{ color: 'var(--text-primary)' }}>{connectedAccount.email}</strong>
          </p>
        )}

        {/* Progress & Stats */}
        {(scanStatus === 'running' || scanProgress) && (
          <div
            style={{
              padding: '16px',
              borderRadius: '8px',
              background: scanStatus === 'error' ? 'rgba(239,68,68,0.08)' : scanStatus === 'done' ? 'rgba(34,197,94,0.08)' : 'rgba(37,99,235,0.08)',
              border: `1px solid ${scanStatus === 'error' ? 'rgba(239,68,68,0.2)' : scanStatus === 'done' ? 'rgba(34,197,94,0.25)' : 'rgba(37,99,235,0.2)'}`,
              marginBottom: '16px',
            }}
            role="status"
            aria-live="polite"
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: scanProgress ? '10px' : '0' }}>
              {scanStatus === 'running' && <Loader2 size={16} className="animate-spin" color="var(--accent-primary)" />}
              {scanStatus === 'done' && <CheckCircle size={16} color="var(--success)" />}
              {scanStatus === 'error' && <AlertCircle size={16} color="var(--error)" />}
              <span style={{ fontSize: '13px', fontWeight: 500, color: scanStatus === 'error' ? 'var(--error)' : 'inherit' }}>
                {scanStatus === 'running' ? 'Scanning emails…' : scanStatus === 'done' ? 'Scan complete!' : 'Scan failed.'}
                {scanProgress && ` (${scanProgress.processed} / ${scanProgress.total})`}
              </span>
            </div>
            
            {scanProgress && (
              <>
                {scanStatus === 'running' && (
                  <div style={{ width: '100%', height: '4px', background: 'var(--bg-secondary)', borderRadius: '2px', overflow: 'hidden' }}>
                    <div
                      style={{
                        height: '100%',
                        width: `${scanProgress.total > 0 ? Math.round((scanProgress.processed / scanProgress.total) * 100) : 0}%`,
                        background: 'var(--accent-primary)',
                        borderRadius: '2px',
                        transition: 'width 0.3s ease',
                      }}
                    />
                  </div>
                )}
                
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: '12px', marginTop: '16px' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <Mail size={14} color="var(--text-muted)" />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Emails Fetched: </span>
                    <strong style={{ fontSize: '12px', color: 'var(--text-primary)' }}>{scanProgress.total ?? 0}</strong>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <ScanLine size={14} color="var(--accent-primary)" />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Emails Processed: </span>
                    <strong style={{ fontSize: '12px', color: 'var(--text-primary)' }}>{scanProgress.processed ?? 0}</strong>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <CheckCircle size={14} color="var(--success)" />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Transactions: </span>
                    <strong style={{ fontSize: '12px', color: 'var(--text-primary)' }}>{scanProgress.transactions_found ?? 0}</strong>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <FileText size={14} color="var(--accent-primary)" />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Statements: </span>
                    <strong style={{ fontSize: '12px', color: 'var(--text-primary)' }}>{scanProgress.statements_found ?? 0}</strong>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <Ban size={14} color="var(--text-muted)" />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Non-Financial: </span>
                    <strong style={{ fontSize: '12px', color: 'var(--text-primary)' }}>{scanProgress.non_financial ?? 0}</strong>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <AlertTriangle size={14} color={scanProgress.errors > 0 ? "var(--error)" : "var(--text-muted)"} />
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>Errors: </span>
                    <strong style={{ fontSize: '12px', color: scanProgress.errors > 0 ? 'var(--error)' : 'var(--text-primary)' }}>{scanProgress.errors ?? 0}</strong>
                  </div>
                </div>
              </>
            )}
          </div>
        )}

        {/* Removed redundant done banner, now handled by the Progress & Stats block */}

        {scanStatus === 'error' && scanError && (
          <div
            style={{
              display: 'flex', alignItems: 'center', gap: '10px',
              padding: '12px 16px', borderRadius: '8px',
              background: 'rgba(239,68,68,0.08)', border: '1px solid rgba(239,68,68,0.2)',
              marginBottom: '16px', fontSize: '13px', color: '#ef4444',
            }}
          >
            <AlertCircle size={16} />
            <span>{scanError}</span>
          </div>
        )}

        {/* Action buttons */}
        <div style={{ display: 'flex', gap: '12px' }}>
          <button
            id="start-mail-scan-btn"
            className="btn btn-primary"
            onClick={handleStartScan}
            disabled={!connectedAccount || scanStatus === 'running' || !scanStartDate || !scanEndDate}
          >
            {scanStatus === 'running' ? (
              <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <Loader2 size={16} className="animate-spin" /> Scanning…
              </span>
            ) : (
              <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <ScanLine size={16} /> Start Scan
              </span>
            )}
          </button>
          {(scanStatus === 'done' || scanStatus === 'error') && (
            <button className="btn btn-secondary" onClick={handleCancelScan}>
              Clear
            </button>
          )}
        </div>
      </div>

      {/* ── Secure Backup Recovery Phrase ─────────────────────────────────── */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <KeyRound className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Secure Backup Recovery Phrase</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '20px' }}>
          Opt-in only — most people restoring via Migration Assistant or iCloud Keychain sync
          never need this. It exists for the rare case where you lose both your Mac and your
          Keychain with no other backup path.
        </p>

        {recoveryPhrase ? (
          <div
            style={{
              padding: '16px',
              borderRadius: '8px',
              background: 'rgba(245,158,11,0.08)',
              border: '1px solid rgba(245,158,11,0.25)',
              marginBottom: '12px',
            }}
          >
            <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '8px' }}>
              Write these 24 words down and store them somewhere as secure as your financial data.
              Anyone with this phrase can decrypt your data on any computer.
            </p>
            <p style={{ fontFamily: 'monospace', fontSize: '13px', lineHeight: 1.8, color: 'var(--text-primary)', wordBreak: 'break-word' }}>
              {recoveryPhrase}
            </p>
          </div>
        ) : (
          <button
            className="btn btn-secondary"
            onClick={handleViewRecoveryPhrase}
            disabled={isFetchingPhrase}
          >
            {isFetchingPhrase ? (
              <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <Loader2 size={16} className="animate-spin" /> Generating…
              </span>
            ) : (
              'View Recovery Phrase'
            )}
          </button>
        )}
      </div>

      {/* ── License & Billing ─────────────────────────────────────────── */}
      <div
        className="glass-panel"
        style={{
          padding: '24px',
          borderColor:
            licenseStatus?.state === 'LOCKED'
              ? 'rgba(239,68,68,0.3)'
              : licenseStatus?.state === 'GRACE'
              ? 'rgba(245,158,11,0.3)'
              : undefined,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <CreditCard className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">License &amp; Billing</h3>
        </div>

        {isLoadingLicense ? (
          <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Loading license status…</p>
        ) : licenseStatus ? (
          <>
            {(licenseStatus.state === 'LOCKED' || licenseStatus.state === 'GRACE') && (
              <div
                style={{
                  display: 'flex', alignItems: 'center', gap: '10px',
                  padding: '12px 16px', borderRadius: '8px',
                  background: licenseStatus.state === 'LOCKED' ? 'rgba(239,68,68,0.08)' : 'rgba(245,158,11,0.08)',
                  border: `1px solid ${licenseStatus.state === 'LOCKED' ? 'rgba(239,68,68,0.2)' : 'rgba(245,158,11,0.2)'}`,
                  marginBottom: '16px', fontSize: '13px',
                  color: licenseStatus.state === 'LOCKED' ? '#ef4444' : '#b45309',
                }}
                role="alert"
              >
                <AlertTriangle size={16} />
                <span>
                  {licenseStatus.state === 'LOCKED'
                    ? 'Your license is locked. Paid features are unavailable until you refresh or reactivate.'
                    : `Your subscription is in its grace period${
                        licenseStatus.days_remaining != null
                          ? ` (${licenseStatus.days_remaining} day${licenseStatus.days_remaining === 1 ? '' : 's'} remaining)`
                          : ''
                      } — refresh once your payment is resolved.`}
                </span>
              </div>
            )}

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: '12px', marginBottom: '20px' }}>
              <div>
                <span style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                  Status
                </span>
                <p
                  style={{
                    fontSize: '14px', fontWeight: 600, marginTop: '4px',
                    color: licenseStatus.state === 'LOCKED' ? '#ef4444' : licenseStatus.is_active ? 'var(--success)' : 'var(--text-primary)',
                  }}
                >
                  {licenseStatus.state || 'Unknown'}
                </p>
              </div>
              {licenseStatus.plan_id && (
                <div>
                  <span style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                    Plan
                  </span>
                  <p style={{ fontSize: '14px', fontWeight: 600, marginTop: '4px' }}>
                    {licenseStatus.plan_id}
                    {licenseStatus.billing_interval ? ` (${licenseStatus.billing_interval})` : ''}
                  </p>
                </div>
              )}
              {licenseStatus.days_remaining != null && licenseStatus.state !== 'GRACE' && (
                <div>
                  <span style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                    {licenseStatus.state === 'TRIAL' || licenseStatus.state === 'TRIALING' ? 'Trial Days Left' : 'Days Remaining'}
                  </span>
                  <p style={{ fontSize: '14px', fontWeight: 600, marginTop: '4px' }}>{licenseStatus.days_remaining}</p>
                </div>
              )}
              {licenseStatus.expiry_date && (
                <div>
                  <span style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                    Renews / Expires
                  </span>
                  <p style={{ fontSize: '14px', fontWeight: 600, marginTop: '4px' }}>
                    {new Date(licenseStatus.expiry_date).toLocaleDateString()}
                  </p>
                </div>
              )}
            </div>

            {licenseActionError && (
              <div
                style={{
                  padding: '12px 16px', borderRadius: '8px',
                  background: 'rgba(239,68,68,0.08)', border: '1px solid rgba(239,68,68,0.2)',
                  marginBottom: '16px', fontSize: '13px', color: '#ef4444',
                }}
              >
                {licenseActionError}
              </div>
            )}

            <div style={{ display: 'flex', gap: '12px', flexWrap: 'wrap' }}>
              <button className="btn btn-secondary" onClick={handleRefreshLicense} disabled={isRefreshingLicense}>
                {isRefreshingLicense ? (
                  <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <Loader2 size={16} className="animate-spin" /> Refreshing…
                  </span>
                ) : (
                  <span style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <RefreshCw size={16} /> Refresh License
                  </span>
                )}
              </button>

              {licenseStatus.is_active ? (
                <button
                  className="btn btn-secondary"
                  onClick={handleDeactivateLicense}
                  disabled={isDeactivating}
                  style={{ color: '#ef4444' }}
                >
                  {isDeactivating ? 'Deactivating…' : 'Deactivate License'}
                </button>
              ) : (
                <button className="btn btn-primary" onClick={() => setShowActivateForm((v) => !v)}>
                  {showActivateForm ? 'Cancel' : 'Activate License'}
                </button>
              )}
            </div>

            {showActivateForm && !licenseStatus.is_active && (
              <div
                style={{
                  marginTop: '20px', paddingTop: '20px', borderTop: '1px solid var(--border)',
                  display: 'flex', flexDirection: 'column', gap: '12px',
                }}
              >
                <p style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
                  Enter the payment confirmation details from your Razorpay checkout to activate this device.
                </p>
                <input
                  className="form-input"
                  placeholder="Email"
                  value={activateEmail}
                  onChange={(e) => setActivateEmail(e.target.value)}
                  style={{ padding: '8px 12px', borderRadius: '8px', background: 'var(--bg-secondary)', border: '1px solid var(--border)', color: 'var(--text-primary)', fontSize: '13px' }}
                />
                <input
                  className="form-input"
                  placeholder="Razorpay Payment ID"
                  value={activatePaymentId}
                  onChange={(e) => setActivatePaymentId(e.target.value)}
                  style={{ padding: '8px 12px', borderRadius: '8px', background: 'var(--bg-secondary)', border: '1px solid var(--border)', color: 'var(--text-primary)', fontSize: '13px' }}
                />
                <input
                  className="form-input"
                  placeholder="Razorpay Signature"
                  value={activateSignature}
                  onChange={(e) => setActivateSignature(e.target.value)}
                  style={{ padding: '8px 12px', borderRadius: '8px', background: 'var(--bg-secondary)', border: '1px solid var(--border)', color: 'var(--text-primary)', fontSize: '13px' }}
                />
                <select
                  className="form-select"
                  aria-label="Billing interval"
                  value={activateBillingInterval}
                  onChange={(e) => setActivateBillingInterval(e.target.value)}
                >
                  <option value="monthly">Monthly</option>
                  <option value="yearly">Yearly</option>
                </select>
                <button
                  className="btn btn-primary"
                  onClick={handleActivateLicense}
                  disabled={isActivating || !activateEmail.trim() || !activatePaymentId.trim() || !activateSignature.trim()}
                >
                  {isActivating ? 'Activating…' : 'Confirm Activation'}
                </button>
              </div>
            )}
          </>
        ) : (
          <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Could not load license status.</p>
        )}
      </div>

      {/* ── Spending Limits ────────────────────────────────────────────── */}
      {/* G16 fix: moved out of the top-level nav (where it competed for
          attention with core ingestion/reconciliation flows) into a Settings
          subsection — the route and page are unchanged, only the entry point. */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <Gauge className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Spending Limits</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '16px' }}>
          Configure your global monthly limit, per-category budgets, and alert thresholds.
        </p>
        <Button onClick={() => navigate('/spending-limits')} aria-label="Manage spending limits">
          Manage Spending Limits
        </Button>
      </div>

      {/* ── Appearance ─────────────────────────────────────────────────── */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <Palette className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Appearance</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '16px' }}>
          Dinero currently ships dark-mode only. Light mode is planned for a future release.
        </p>
        <div style={{ display: 'flex', gap: '16px' }}>
          <button className="btn btn-primary" style={{ flex: 1, cursor: 'default' }} disabled aria-label="Dark Mode (currently active)">
            Dark Mode (Active)
          </button>
          <button className="btn btn-secondary" style={{ flex: 1 }} disabled aria-label="Light Mode (coming soon)">
            Light Mode (Coming Soon)
          </button>
        </div>
      </div>

      {/* ── LLM Configuration ──────────────────────────────────────────── */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <Cpu className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">LLM Configuration</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '20px' }}>
          Select the local AI model for parsing statements. Heavier models
          perform better but require more RAM.
        </p>
        <select
          value={model}
          onChange={handleModelChange}
          className="form-select"
          aria-label="Local AI model for statement parsing"
        >
          {availableModels.map((m) => (
            <option key={m.id} value={m.id}>
              {m.name} (Requires {m.min_ram_gb}GB+ RAM, ~{m.approx_size_gb}GB)
            </option>
          ))}
        </select>
      </div>

      {/* ── Network Activity ─────────────────────────────────────────────── */}
      <NetworkActivity />

      {/* ── Privacy & Consent History (TASK-FE-014) ──────────────────── */}
      <PrivacySettings />

      {/* ── Pattern Rules (G14 fix) ───────────────────────────────────── */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <Wand2 className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Pattern Rules</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '16px' }}>
          Learned extraction rules per bank/field. Disable a rule if it's misfiring — extraction
          falls back to the standard pipeline for that field until you re-enable it.
        </p>

        {isLoadingRules ? (
          <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Loading…</p>
        ) : patternRules.length === 0 ? (
          <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>No pattern rules learned yet.</p>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', maxHeight: '280px', overflowY: 'auto' }}>
            {patternRules.map((rule) => (
              <div
                key={rule.id}
                style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px',
                  padding: '10px 14px', borderRadius: '8px',
                  background: 'var(--bg-secondary)', border: '1px solid var(--border)', fontSize: '12px',
                }}
              >
                <div>
                  <strong style={{ color: 'var(--text-primary)' }}>{rule.merchant_id}</strong>
                  <span style={{ color: 'var(--text-muted)' }}> — {rule.pattern_value}</span>
                  <div style={{ color: 'var(--text-muted)', marginTop: '2px' }}>
                    {rule.success_count} succeeded / {rule.failure_count} failed
                  </div>
                </div>
                <button
                  className="btn btn-secondary"
                  style={{ padding: '6px 12px', fontSize: '12px', color: rule.is_active ? '#ef4444' : undefined }}
                  onClick={() => handleToggleRule(rule)}
                  disabled={togglingRuleId === rule.id}
                >
                  {togglingRuleId === rule.id ? '…' : rule.is_active ? 'Disable' : 'Enable'}
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* ── Statement Passwords (TASK-FE-015) ─────────────────────────── */}
      <StatementPasswordSettings />

      {/* ── Data Management ────────────────────────────────────────────── */}
      {/* G4/J7 fix: Settings had no data export or restore-from-backup UI at
          all. Diagnostic bundle export and the delete-my-data flow moved
          into PrivacySettings (TASK-FE-014) above — this section keeps only
          the full financial-data export/restore, which is distinct from
          both. */}
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <FileText className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Data Management</h3>
        </div>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '20px' }}>
          Export a full encrypted copy of your financial data, or restore from your most recent backup.
        </p>
        <div style={{ display: 'flex', gap: '12px', flexWrap: 'wrap' }}>
          <button className="btn btn-secondary" onClick={handleExportFullData} disabled={isExportingData}>
            <FileText size={18} />
            {isExportingData ? 'Exporting…' : 'Export My Data (Encrypted)'}
          </button>
          <button className="btn btn-secondary" onClick={handleRestoreFromBackup} disabled={isRestoring}>
            <RefreshCw size={18} />
            {isRestoring ? 'Restoring…' : 'Restore from Backup'}
          </button>
        </div>
      </div>
    </div>
  );
}
