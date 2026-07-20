import { useState, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Mail, Palette, Cpu, CheckCircle, ScanLine, Loader2, CalendarRange, AlertCircle, FileText, Ban, AlertTriangle, KeyRound, CreditCard, RefreshCw, Wand2, Gauge, Shield, Users, Settings as SettingsIcon } from 'lucide-react';
import { API, LlmModelInfo, LicenseStatusResponse, PatternRuleHealth } from '../lib/ipc';
import { useGlobalState } from '../lib/GlobalStateContext';
import { cn } from '@/lib/utils';
import { SidebarNavItem } from '@/components/ui/sidebar-nav-item';
import { Button } from '@/components/ui/button';

import NetworkActivity from '../components/NetworkActivity';
import PrivacySettings from '../components/settings/PrivacySettings';
import ConnectedAccountsSettings from '../components/settings/ConnectedAccountsSettings';
import StatementPasswordSettings from '../components/settings/StatementPasswordSettings';
import MenuBarExtraSettings from '../components/settings/MenuBarExtraSettings';
import LifecycleSettings from '../components/settings/LifecycleSettings';
import BudgetsSettings from '../components/settings/BudgetsSettings';
import LocalLlmSettings from '../components/settings/LocalLlmSettings';

type SettingsSection = 'budgets' | 'accounts' | 'privacy' | 'license' | 'advanced' | 'appearance';

const SETTINGS_SECTIONS = [
  { id: 'budgets', label: 'Budgets', icon: Gauge },
  { id: 'accounts', label: 'Connected Accounts', icon: Users },
  { id: 'privacy', label: 'Privacy & Security', icon: Shield },
  { id: 'license', label: 'License & Billing', icon: CreditCard },
  { id: 'advanced', label: 'Advanced', icon: SettingsIcon },
  { id: 'appearance', label: 'Appearance', icon: Palette },
] as const;

export default function Settings() {
  const [searchParams, setSearchParams] = useSearchParams();
  const currentSection = (searchParams.get('section') as SettingsSection) || 'budgets';
  const setSection = (section: SettingsSection) => setSearchParams({ section });

  // ── Global State (Mail Scan) ──────────────────────────────────────────
  const {
    scanStartDate, setScanStartDate,
    scanEndDate, setScanEndDate,
    scanStatus, setScanStatus,
    scanProgress, setScanProgress,
    connectedAccounts,
    handleStartScan
  } = useGlobalState();
  const connectedAccount = connectedAccounts[0] ?? null;

  // ── Recovery Phrase ──────────────────────────────────────────────────
  const [recoveryPhrase, setRecoveryPhrase] = useState<string | null>(null);
  const [isFetchingPhrase, setIsFetchingPhrase] = useState(false);

  const handleViewRecoveryPhrase = async () => {
    let confirmed: boolean;
    const warning = 'Only use this if you anticipate losing both your Mac and your Keychain. Anyone who has this 24-word phrase can decrypt your financial data on any computer — it bypasses this Mac\'s hardware-bound protection entirely. Keep it as secure as your data itself.';
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
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      alert('Failed to retrieve recovery phrase: ' + errorMessage);
    } finally {
      setIsFetchingPhrase(false);
    }
  };

  // ── License & Billing ────────────────────────────────────────────────
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
    } catch {
      // Ignore initial load error
    } finally {
      setIsLoadingLicense(false);
    }
  };

  useEffect(() => { loadLicenseStatus(); }, []);

  const handleRefreshLicense = async () => {
    setIsRefreshingLicense(true); setLicenseActionError(null);
    try { await API.licensing.refresh(); await loadLicenseStatus(); }
    catch (err: unknown) { setLicenseActionError(err instanceof Error ? err.message : String(err)); }
    finally { setIsRefreshingLicense(false); }
  };

  const handleDeactivateLicense = async () => {
    let confirmed: boolean;
    const warning = "Deactivate this device's license? Paid features will be unavailable here until you reactivate.";
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Deactivate License', kind: 'warning' });
    } catch { confirmed = confirm(warning); }
    if (!confirmed) return;

    setIsDeactivating(true); setLicenseActionError(null);
    try { await API.licensing.deactivate(); await loadLicenseStatus(); }
    catch (err: unknown) { setLicenseActionError(err instanceof Error ? err.message : String(err)); }
    finally { setIsDeactivating(false); }
  };

  const handleActivateLicense = async () => {
    setIsActivating(true); setLicenseActionError(null);
    try {
      await API.licensing.activate(activateEmail.trim(), activatePaymentId.trim(), activateSignature.trim(), activateBillingInterval);
      setShowActivateForm(false); setActivateEmail(''); setActivatePaymentId(''); setActivateSignature('');
      await loadLicenseStatus();
    }
    catch (err: unknown) { setLicenseActionError(err instanceof Error ? err.message : String(err)); }
    finally { setIsActivating(false); }
  };

  // ── Pattern Rules ────────────────────────────────────────────────────
  const [patternRules, setPatternRules] = useState<PatternRuleHealth[]>([]);
  const [isLoadingRules, setIsLoadingRules] = useState(true);
  const [togglingRuleId, setTogglingRuleId] = useState<string | null>(null);

  const loadPatternRules = async () => {
    setIsLoadingRules(true);
    try { const rules = await API.patternRules.list(); setPatternRules(rules); }
    catch {
      // Ignore initial load error
    } finally { setIsLoadingRules(false); }
  };

  useEffect(() => { loadPatternRules(); }, []);

  const handleToggleRule = async (rule: PatternRuleHealth) => {
    setTogglingRuleId(rule.id);
    try {
      await API.patternRules.setStatus(rule.id, rule.is_active ? 'inactive' : 'active');
      await loadPatternRules();
    } catch (err: unknown) { 
      const errorMessage = err instanceof Error ? err.message : String(err);
      alert('Failed to update pattern rule: ' + errorMessage); 
    }
    finally { setTogglingRuleId(null); }
  };


  return (
    <div className="flex h-full w-full overflow-hidden">
      
      {/* ── Column 2: Navigation (Settings) ─────────────────────────────────── */}
      <div 
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
          <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">
            Settings
          </h1>
        </div>

        <div className="flex-1 overflow-y-auto py-2">
          <nav className="flex flex-col gap-1">
            {SETTINGS_SECTIONS.map((s) => {
              const isSelected = currentSection === s.id;
              return (
                <SidebarNavItem
                  key={s.id}
                  isSelected={isSelected}
                  onClick={() => setSection(s.id)}
                  icon={<s.icon className="w-4 h-4" />}
                  label={s.label}
                />
              );
            })}
          </nav>
        </div>
      </div>

      {/* ── Column 3: Content Area ────────────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12 text-[#064E3B]">
        <div className="max-w-3xl mx-auto space-y-12">
          
          {/* BUDGETS */}
          {currentSection === 'budgets' && (
            <div className="animate-in fade-in duration-300">
              <BudgetsSettings />
            </div>
          )}

          {/* ACCOUNTS & SYNC */}
          {currentSection === 'accounts' && (
            <div className="animate-in fade-in duration-300 space-y-12">
              <section>
                <ConnectedAccountsSettings />
              </section>

              <div className="h-px w-full bg-[#064E3B]/10" />

              <section>
                <div className="mb-6">
                  <h2 className="text-xl font-bold flex items-center gap-2">
                    <ScanLine className="w-5 h-5" /> Mail Scan
                  </h2>
                  <p className="text-sm mt-1 text-[#064E3B]/70">
                    Scan your Gmail inbox for financial emails within a custom date range. The scan runs locally.
                  </p>
                </div>
                
                <div className="space-y-6">
                  {!connectedAccount && (
                    <div className="p-4 rounded-xl bg-amber-500/10 border border-amber-500/20 text-[13px] font-semibold text-amber-700">
                      ⚠️ Connect a Gmail account above before scanning.
                    </div>
                  )}

                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <label htmlFor="scan-start-date" className="text-[11px] font-bold uppercase tracking-wider flex items-center gap-1 text-[#064E3B]/60">
                        <CalendarRange className="w-3.5 h-3.5" /> Start Date
                      </label>
                      <input
                        id="scan-start-date"
                        type="date"
                        value={scanStartDate}
                        onChange={(e) => setScanStartDate(e.target.value)}
                        disabled={scanStatus === 'running' || !connectedAccount}
                        className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B]"
                      />
                    </div>
                    <div className="space-y-2">
                      <label htmlFor="scan-end-date" className="text-[11px] font-bold uppercase tracking-wider flex items-center gap-1 text-[#064E3B]/60">
                        <CalendarRange className="w-3.5 h-3.5" /> End Date
                      </label>
                      <input
                        id="scan-end-date"
                        type="date"
                        value={scanEndDate}
                        onChange={(e) => setScanEndDate(e.target.value)}
                        disabled={scanStatus === 'running' || !connectedAccount}
                        className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B]"
                      />
                    </div>
                  </div>

                  {connectedAccount && (
                    <p className="text-[12px] text-[#064E3B]/60">
                      Scanning account: <strong className="font-semibold text-[#064E3B]">{connectedAccount.email}</strong>
                    </p>
                  )}

                  {(scanStatus === 'running' || scanProgress) && (
                    <div className={cn(
                      "p-5 rounded-xl border",
                      scanStatus === 'error' ? 'bg-red-500/10 border-red-500/20' : scanStatus === 'done' ? 'bg-emerald-500/10 border-emerald-500/20' : 'bg-[#064E3B]/5 border-[#064E3B]/10'
                    )}>
                      <div className="flex items-center gap-2 mb-4 font-semibold text-[14px]">
                        {scanStatus === 'running' && <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]" />}
                        {scanStatus === 'done' && <CheckCircle className="w-4 h-4 text-emerald-600" />}
                        {scanStatus === 'error' && <AlertCircle className="w-4 h-4 text-red-600" />}
                        <span className={scanStatus === 'error' ? 'text-red-600' : 'text-[#064E3B]'}>
                          {scanStatus === 'running' ? 'Scanning emails…' : scanStatus === 'done' ? 'Scan complete!' : 'Scan failed.'}
                          {scanProgress && ` (${scanProgress.processed} / ${scanProgress.total})`}
                        </span>
                      </div>
                      
                      {scanProgress && scanStatus === 'running' && (
                        <div className="w-full h-1.5 rounded-full overflow-hidden bg-[#064E3B]/10 mb-5">
                          <div
                            className="h-full bg-[#064E3B] transition-all duration-300"
                            style={{ width: `${scanProgress.total > 0 ? (scanProgress.processed / scanProgress.total) * 100 : 0}%` }}
                          />
                        </div>
                      )}
                      
                      {scanProgress && (
                        <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
                          <div className="flex items-center gap-2 text-[13px]">
                            <Mail className="w-4 h-4 text-[#064E3B]/50" />
                            <span className="text-[#064E3B]/60 font-medium">Fetched:</span>
                            <strong className="font-bold">{scanProgress.total ?? 0}</strong>
                          </div>
                          <div className="flex items-center gap-2 text-[13px]">
                            <ScanLine className="w-4 h-4 text-[#064E3B]/50" />
                            <span className="text-[#064E3B]/60 font-medium">Processed:</span>
                            <strong className="font-bold">{scanProgress.processed ?? 0}</strong>
                          </div>
                          <div className="flex items-center gap-2 text-[13px]">
                            <CheckCircle className="w-4 h-4 text-emerald-600" />
                            <span className="text-[#064E3B]/60 font-medium">Txns:</span>
                            <strong className="font-bold">{scanProgress.transactions_found ?? 0}</strong>
                          </div>
                          <div className="flex items-center gap-2 text-[13px]">
                            <FileText className="w-4 h-4 text-[#064E3B]/50" />
                            <span className="text-[#064E3B]/60 font-medium">Statements:</span>
                            <strong className="font-bold">{scanProgress.statements_found ?? 0}</strong>
                          </div>
                          <div className="flex items-center gap-2 text-[13px]">
                            <Ban className="w-4 h-4 text-[#064E3B]/50" />
                            <span className="text-[#064E3B]/60 font-medium">Ignored:</span>
                            <strong className="font-bold">{scanProgress.non_financial ?? 0}</strong>
                          </div>
                          <div className="flex items-center gap-2 text-[13px]">
                            <AlertTriangle className={cn("w-4 h-4", scanProgress.errors > 0 ? "text-red-600" : "text-[#064E3B]/50")} />
                            <span className="text-[#064E3B]/60 font-medium">Errors:</span>
                            <strong className={scanProgress.errors > 0 ? 'text-red-600 font-bold' : 'font-bold'}>{scanProgress.errors ?? 0}</strong>
                          </div>
                        </div>
                      )}
                    </div>
                  )}

                  <div className="flex gap-3">
                    <Button onClick={handleStartScan} disabled={!connectedAccount || scanStatus === 'running' || !scanStartDate || !scanEndDate} className="h-9 px-4 font-semibold" style={{ background: '#064E3B', color: '#F8E7C9' }}>
                      {scanStatus === 'running' ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <ScanLine className="w-4 h-4 mr-2" />}
                      Start Scan
                    </Button>
                    {(scanStatus === 'done' || scanStatus === 'error') && (
                      <Button variant="outline" className="h-9 px-4 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5" onClick={() => { setScanStatus('idle'); setScanProgress(null); }}>Clear</Button>
                    )}
                  </div>
                </div>
              </section>
            </div>
          )}

          {/* PRIVACY & SECURITY */}
          {currentSection === 'privacy' && (
            <div className="animate-in fade-in duration-300 space-y-12">
              <section>
                <PrivacySettings />
              </section>
              <div className="h-px w-full bg-[#064E3B]/10" />
              <section>
                <StatementPasswordSettings />
              </section>
              <div className="h-px w-full bg-[#064E3B]/10" />
              <section>
                <div className="mb-6">
                  <h2 className="text-xl font-bold flex items-center gap-2">
                    <KeyRound className="w-5 h-5" /> Secure Backup Recovery Phrase
                  </h2>
                  <p className="text-sm mt-1 text-[#064E3B]/70">
                    Opt-in only. Exists for the rare case where you lose both your Mac and your Keychain.
                  </p>
                </div>
                {recoveryPhrase ? (
                  <div className="p-5 rounded-xl bg-amber-500/10 border border-amber-500/20 mb-4">
                    <p className="text-[13px] font-semibold text-amber-700 mb-3">Write these 24 words down. Anyone with this phrase can decrypt your data on any computer.</p>
                    <p className="font-mono text-[14px] leading-relaxed text-[#064E3B] font-medium break-words select-all p-4 bg-[#F8E7C9] rounded-lg border border-[#064E3B]/20 shadow-inner">
                      {recoveryPhrase}
                    </p>
                  </div>
                ) : (
                  <Button variant="outline" className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5" onClick={handleViewRecoveryPhrase} disabled={isFetchingPhrase}>
                    {isFetchingPhrase ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : null}
                    {isFetchingPhrase ? 'Generating…' : 'View Recovery Phrase'}
                  </Button>
                )}
              </section>
              <div className="h-px w-full bg-[#064E3B]/10" />
              <section>
                <LifecycleSettings />
              </section>
            </div>
          )}

          {/* LICENSE & BILLING */}
          {currentSection === 'license' && (
            <div className="animate-in fade-in duration-300 space-y-6">
              <div className="mb-6">
                <h2 className="text-xl font-bold flex items-center gap-2">
                  <CreditCard className="w-5 h-5" /> License & Billing
                </h2>
              </div>

              {isLoadingLicense ? (
                <div className="py-8 flex justify-center"><Loader2 className="w-5 h-5 animate-spin text-[#064E3B]/50" /></div>
              ) : licenseStatus ? (
                <div className="space-y-6">
                  {(licenseStatus.state === 'LOCKED' || licenseStatus.state === 'GRACE') && (
                    <div className={cn("p-5 rounded-xl border flex items-center gap-3 text-[14px] font-medium", licenseStatus.state === 'LOCKED' ? 'bg-red-500/10 border-red-500/20 text-red-700' : 'bg-amber-500/10 border-amber-500/20 text-amber-700')}>
                      <AlertTriangle className="w-5 h-5 shrink-0" />
                      <p>
                        {licenseStatus.state === 'LOCKED' 
                          ? 'Your license is locked. Paid features are unavailable until you refresh or reactivate.'
                          : `Your subscription is in its grace period${licenseStatus.days_remaining != null ? ` (${licenseStatus.days_remaining} day${licenseStatus.days_remaining === 1 ? '' : 's'} remaining)` : ''} — refresh once your payment is resolved.`}
                      </p>
                    </div>
                  )}

                  <div className="grid grid-cols-2 sm:grid-cols-4 gap-6 p-6 rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50">
                    <div>
                      <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">Status</p>
                      <p className={cn("text-[15px] font-bold", licenseStatus.state === 'LOCKED' ? 'text-red-600' : licenseStatus.is_active ? 'text-emerald-600' : 'text-[#064E3B]')}>
                        {licenseStatus.state || 'Unknown'}
                      </p>
                    </div>
                    {licenseStatus.plan_id && (
                      <div>
                        <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">Plan</p>
                        <p className="text-[15px] font-bold">
                          {licenseStatus.plan_id} {licenseStatus.billing_interval ? `(${licenseStatus.billing_interval})` : ''}
                        </p>
                      </div>
                    )}
                    {licenseStatus.days_remaining != null && licenseStatus.state !== 'GRACE' && (
                      <div>
                        <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">
                          {licenseStatus.state === 'TRIAL' || licenseStatus.state === 'TRIALING' ? 'Trial Left' : 'Remaining'}
                        </p>
                        <p className="text-[15px] font-bold">{licenseStatus.days_remaining} Days</p>
                      </div>
                    )}
                    {licenseStatus.expiry_date && (
                      <div>
                        <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">Renews</p>
                        <p className="text-[15px] font-bold">{new Date(licenseStatus.expiry_date).toLocaleDateString()}</p>
                      </div>
                    )}
                  </div>

                  {licenseActionError && (
                    <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 text-[13px] font-medium text-red-700">
                      {licenseActionError}
                    </div>
                  )}

                  <div className="flex flex-wrap gap-3">
                    <Button variant="outline" className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5" onClick={handleRefreshLicense} disabled={isRefreshingLicense}>
                      {isRefreshingLicense ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <RefreshCw className="w-4 h-4 mr-2" />} Refresh License
                    </Button>
                    
                    {licenseStatus.is_active ? (
                      <Button variant="outline" className="h-9 font-semibold border-red-200 text-red-600 hover:text-red-700 hover:bg-red-50 hover:border-red-300" onClick={handleDeactivateLicense} disabled={isDeactivating}>
                        {isDeactivating ? 'Deactivating…' : 'Deactivate License'}
                      </Button>
                    ) : (
                      <Button className="h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90" onClick={() => setShowActivateForm(v => !v)}>
                        {showActivateForm ? 'Cancel' : 'Activate License'}
                      </Button>
                    )}
                  </div>

                  {showActivateForm && !licenseStatus.is_active && (
                    <div className="mt-8 pt-6 border-t border-[#064E3B]/10 space-y-4">
                      <p className="text-[13px] font-medium text-[#064E3B]/70">Enter the payment confirmation details from your Razorpay checkout.</p>
                      <div className="grid gap-4 max-w-sm">
                        <input className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B] placeholder:text-[#064E3B]/40" placeholder="Email" value={activateEmail} onChange={e => setActivateEmail(e.target.value)} />
                        <input className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B] placeholder:text-[#064E3B]/40" placeholder="Payment ID" value={activatePaymentId} onChange={e => setActivatePaymentId(e.target.value)} />
                        <input className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B] placeholder:text-[#064E3B]/40" placeholder="Signature" value={activateSignature} onChange={e => setActivateSignature(e.target.value)} />
                        <select className="w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B]" value={activateBillingInterval} onChange={e => setActivateBillingInterval(e.target.value)}>
                          <option value="monthly">Monthly</option>
                          <option value="yearly">Yearly</option>
                        </select>
                        <Button className="h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90" onClick={handleActivateLicense} disabled={isActivating || !activateEmail.trim() || !activatePaymentId.trim() || !activateSignature.trim()}>
                          {isActivating ? 'Activating…' : 'Confirm Activation'}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              ) : (
                <p className="text-[13px] font-medium text-[#064E3B]/70">Could not load license status.</p>
              )}
            </div>
          )}

          {/* ADVANCED */}
          {currentSection === 'advanced' && (
            <div className="animate-in fade-in duration-300 space-y-12">
              <LocalLlmSettings />

              <div className="h-px w-full bg-[#064E3B]/10" />

              <section>
                <div className="mb-6">
                  <h2 className="text-xl font-bold flex items-center gap-2">
                    <Wand2 className="w-5 h-5" /> Pattern Rules
                  </h2>
                  <p className="text-sm mt-1 text-[#064E3B]/70">
                    Learned extraction rules per bank/field. Disable a rule if it's misfiring.
                  </p>
                </div>
                
                {isLoadingRules ? (
                  <p className="text-[13px] font-medium text-[#064E3B]/70">Loading…</p>
                ) : patternRules.length === 0 ? (
                  <p className="text-[13px] font-medium text-[#064E3B]/70">No pattern rules learned yet.</p>
                ) : (
                  <div className="space-y-3 max-h-[400px] overflow-y-auto pr-2">
                    {patternRules.map((rule) => (
                      <div key={rule.id} className="p-4 rounded-xl border border-[#064E3B]/10 flex items-center justify-between gap-4 bg-[#F8E7C9]/50">
                        <div className="min-w-0 flex-1">
                          <p className="text-[14px] font-bold truncate text-[#064E3B]">{rule.merchant_id} <span className="text-[#064E3B]/50 mx-1">—</span> {rule.pattern_type}</p>
                          <p className="text-[12px] font-medium mt-1 truncate text-[#064E3B]/70">{rule.pattern_value}</p>
                        </div>
                        <Button
                          variant="outline"
                          className={cn("h-8 text-[12px] font-semibold border-[#064E3B]/20 flex-shrink-0", rule.is_active ? "text-red-600 border-red-200 hover:bg-red-50 hover:text-red-700 hover:border-red-300" : "text-[#064E3B] hover:bg-[#064E3B]/5")}
                          onClick={() => handleToggleRule(rule)}
                          disabled={togglingRuleId === rule.id}
                        >
                          {togglingRuleId === rule.id ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : rule.is_active ? 'Disable' : 'Enable'}
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
              </section>

              <div className="h-px w-full bg-[#064E3B]/10" />

              <section>
                <NetworkActivity />
              </section>
            </div>
          )}

          {/* APPEARANCE */}
          {currentSection === 'appearance' && (
            <div className="animate-in fade-in duration-300 space-y-12">
              <section>
                <div className="mb-6">
                  <h2 className="text-xl font-bold flex items-center gap-2">
                    <Palette className="w-5 h-5" /> Appearance
                  </h2>
                  <p className="text-sm mt-1 text-[#064E3B]/70">
                    Dinero currently ships dark-mode only. Light mode is planned for a future release.
                  </p>
                </div>
                <div className="flex gap-4 max-w-sm p-1 rounded-xl bg-[#064E3B]/5 border border-[#064E3B]/10">
                  <Button className="flex-1 cursor-default h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] shadow-sm rounded-lg" disabled>
                    Dark Mode (Active)
                  </Button>
                  <Button variant="ghost" className="flex-1 cursor-default h-9 font-semibold text-[#064E3B]/60 hover:text-[#064E3B]/60 hover:bg-transparent" disabled>
                    Light Mode
                  </Button>
                </div>
              </section>

              <div className="h-px w-full bg-[#064E3B]/10" />

              <section>
                <MenuBarExtraSettings />
              </section>
            </div>
          )}

        </div>
      </div>
    </div>
  );
}
