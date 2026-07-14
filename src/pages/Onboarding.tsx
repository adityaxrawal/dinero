import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Loader2, Mail, ShieldCheck, AlertCircle, Check } from 'lucide-react';
import { API, LlmModelInfo } from '@/lib/ipc';
import { OUTBOUND_CHANNEL_DISCLOSURE, BETA_PROGRAM_DISCLOSURE } from '@/constants/privacy';

const TOTAL_STEPS = 3;

// TASK-AUTH-003 (Document 18 §4.21a): `disclosure_text` must be the exact
// verbatim text shown to the user at consent time, not a paraphrase of what
// they did — this is that verbatim text, assembled from the same constants
// the consent screen itself renders from.
const GMAIL_CONSENT_DISCLOSURE_TEXT = [
  'Requested Gmail scope: https://www.googleapis.com/auth/gmail.readonly',
  ...BETA_PROGRAM_DISCLOSURE,
  ...OUTBOUND_CHANNEL_DISCLOSURE,
].join(' | ');

type StatementPref = 'auto' | 'manual';

export default function Onboarding() {
  const navigate = useNavigate();
  const [step, setStep] = useState(1);
  const [loading, setLoading] = useState(false);
  const [oauthError, setOauthError] = useState<string | null>(null);

  // Form state — Step 1
  const [timezone, setTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);
  const [monthlyLimit, setMonthlyLimit] = useState('50000');
  const [limitError, setLimitError] = useState<string | null>(null);
  const [statementPref, setStatementPref] = useState<StatementPref>('auto');

  // Form state — Step 2
  const [scanRange, setScanRange] = useState('3');
  // Doc 16 §12.3: the 5-tier model catalog, fetched from the backend — not
  // hardcoded here, so this can never drift from src-tauri's own list again.
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [llmConfig, setLlmConfig] = useState('gemma4_e4b');
  useEffect(() => {
    API.llm.getAvailableModels()
      .then((models) => {
        setAvailableModels(models);
        // Default to the lowest-tier (broadest-compatibility) model.
        if (models.length > 0) setLlmConfig(models[0].id);
      })
      .catch((err) => console.error('Failed to fetch LLM model catalog:', err));
  }, []);

  const handleNext = () => {
    if (step === 1) {
      const parsedLimit = parseFloat(monthlyLimit);
      if (isNaN(parsedLimit) || parsedLimit <= 0) {
        setLimitError('Must be > 0');
        return;
      }
      setLimitError(null);
    }
    setStep((s) => Math.min(s + 1, TOTAL_STEPS));
  };
  const handleBack = () => {
    setOauthError(null);
    setStep((s) => Math.max(s - 1, 1));
  };

  const handleConnectGmail = async () => {
    setLoading(true);
    setOauthError(null);
    try {
      // TASK-AUTH-003 (Document 30): "on consent-screen acknowledgment,
      // insert a gmail_oauth_consent row" — recorded here, at the moment of
      // acknowledgment, before the OAuth round-trip even starts (not tied to
      // whether it later succeeds).
      try {
        await API.privacy.recordConsentEvent('gmail_oauth_consent', GMAIL_CONSENT_DISCLOSURE_TEXT);
      } catch (consentErr) {
        // Non-fatal — never block onboarding on a logging failure.
        console.error('Failed to record Gmail OAuth consent event:', consentErr);
      }

      // Call real OAuth IPC — must succeed before marking onboarded
      await API.auth.startGoogle();
      // Only persist onboarded state after successful token storage
      localStorage.setItem('dinero_onboarded', 'true');
      localStorage.setItem('dinero_monthly_limit', monthlyLimit);
      localStorage.setItem('dinero_scan_range', scanRange);
      localStorage.setItem('llm_model', llmConfig);
      localStorage.setItem('dinero_statement_pref', statementPref);
      await savePreferencesToBackend();
      navigate('/');
    } catch (e: any) {
      const msg = e?.message || 'Failed to connect Gmail. Please try again.';
      setOauthError(msg);
    } finally {
      setLoading(false);
    }
  };

  // G2 fix: statement-only users previously had no way to finish onboarding
  // without connecting Gmail — this persists the same onboarding state
  // without the OAuth step, for users who selected "Manual" in step 1.
  const handleSkipGmail = async () => {
    setLoading(true);
    try {
      try {
        await API.privacy.recordConsentEvent('onboarding_disclosure', OUTBOUND_CHANNEL_DISCLOSURE.join(' | '));
      } catch (consentErr) {
        console.error('Failed to record onboarding disclosure consent:', consentErr);
      }
      localStorage.setItem('dinero_onboarded', 'true');
      localStorage.setItem('dinero_monthly_limit', monthlyLimit);
      localStorage.setItem('dinero_scan_range', scanRange);
      localStorage.setItem('llm_model', llmConfig);
      localStorage.setItem('dinero_statement_pref', statementPref);
      await savePreferencesToBackend();
      navigate('/');
    } finally {
      setLoading(false);
    }
  };

  // G19 fix: previously these choices only lived in browser localStorage —
  // never persisted to `local_profile`, so they didn't survive a
  // reinstall/reset and `monthlyLimit` in particular never reached the same
  // row Settings → Spending Limits reads from. Best-effort: a persistence
  // hiccup here shouldn't block the user from finishing onboarding, since
  // Settings still offers a normal way to set these afterward.
  const savePreferencesToBackend = async () => {
    try {
      await API.onboarding.savePreferences({
        timezone,
        spendingLimitMonthly: parseFloat(monthlyLimit) || 0,
        historicalScanMonths: parseInt(scanRange, 10) || 3,
        llmModel: llmConfig,
        statementPreference: statementPref,
      });
    } catch (err) {
      console.error('Failed to persist onboarding preferences to backend:', err);
    }
  };

  const stepLabel = `Step ${step} of ${TOTAL_STEPS}`;

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-lg shadow-2xl">
        <CardHeader>
          <div className="flex items-center justify-between mb-1">
            <CardTitle className="text-2xl">Welcome to Dinero</CardTitle>
            <span className="text-xs text-muted-foreground" aria-label={stepLabel}>{stepLabel}</span>
          </div>
          {/* Step progress bar */}
          <div className="w-full h-1.5 bg-secondary/80 rounded-full overflow-hidden" role="progressbar" aria-valuenow={step} aria-valuemin={1} aria-valuemax={TOTAL_STEPS} aria-label="Onboarding progress">
            <div
              className="h-full rounded-full transition-all duration-500 ease-out"
              style={{
                width: `${(step / TOTAL_STEPS) * 100}%`,
                background: '#2563eb',
              }}
            />
          </div>
          <CardDescription className="mt-2">Let's get your financial command center set up.</CardDescription>
        </CardHeader>

        <CardContent>
          {step === 1 && (
            <div className="space-y-4 animate-in fade-in slide-in-from-bottom-4">
              <div className="space-y-2">
                <Label htmlFor="timezone">Timezone</Label>
                <Input
                  id="timezone"
                  value={timezone}
                  onChange={(e) => setTimezone(e.target.value)}
                  aria-describedby="timezone-hint"
                />
                <p id="timezone-hint" className="text-xs text-muted-foreground">
                  Used for aligning transaction dates correctly.
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="limit">Monthly Spending Limit (₹)</Label>
                <Input
                  id="limit"
                  type="number"
                  min="0"
                  value={monthlyLimit}
                  onChange={(e) => setMonthlyLimit(e.target.value)}
                  aria-describedby="limit-hint"
                />
                <p id="limit-hint" className="text-xs text-muted-foreground">
                  We'll alert you when you approach this limit.
                </p>
                {limitError && <p className="text-xs text-red-700">{limitError}</p>}
              </div>

              <div className="space-y-2">
                <Label>Statement Preference</Label>
                <div className="grid grid-cols-2 gap-3" role="radiogroup" aria-label="Statement preference">
                  {/* Auto (Gmail) option */}
                  <button
                    type="button"
                    role="radio"
                    aria-checked={statementPref === 'auto'}
                    onClick={() => setStatementPref('auto')}
                    className={[
                      'relative flex flex-col items-center p-5 rounded-xl border-[1.5px] text-sm transition-all duration-200 ease-out outline-none',
                      'focus-visible:ring-2 focus-visible:ring-[#2563eb]/60 focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                      'hover:-translate-y-0.5',
                      statementPref === 'auto'
                        ? [
                            'border-[#2563eb]/70 font-semibold',
                            'bg-[#2563eb]/8',
                            'shadow-[0_0_0_1px_rgba(37,99,235,0.2)]',
                            'text-[#1d4ed8]',
                          ].join(' ')
                        : 'border-border bg-background text-foreground hover:border-[#2563eb]/35 hover:bg-[#2563eb]/[0.04]',
                    ].join(' ')}
                  >
                    {/* Checkmark badge — visible only when selected */}
                    <span
                      aria-hidden="true"
                      className={[
                        'absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center',
                        'bg-[#2563eb]',
                        'transition-all duration-200 ease-out',
                        statementPref === 'auto' ? 'opacity-100 scale-100' : 'opacity-0 scale-0',
                      ].join(' ')}
                    >
                      <Check className="w-3 h-3 text-white" strokeWidth={3} />
                    </span>
                    <Mail
                      className={['w-6 h-6 mb-2 transition-colors duration-200', statementPref === 'auto' ? 'text-[#2563eb]' : 'text-muted-foreground'].join(' ')}
                      aria-hidden="true"
                    />
                    <span className="font-medium">Auto (Gmail)</span>
                    <span className={['text-xs mt-0.5 transition-colors', statementPref === 'auto' ? 'text-[#1d4ed8]' : 'text-muted-foreground'].join(' ')}>
                      Fetched from email
                    </span>
                  </button>

                  {/* Manual option */}
                  <button
                    type="button"
                    role="radio"
                    aria-checked={statementPref === 'manual'}
                    onClick={() => setStatementPref('manual')}
                    className={[
                      'relative flex flex-col items-center p-5 rounded-xl border-[1.5px] text-sm transition-all duration-200 ease-out outline-none',
                      'focus-visible:ring-2 focus-visible:ring-[#2563eb]/60 focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                      'hover:-translate-y-0.5',
                      statementPref === 'manual'
                        ? [
                            'border-[#2563eb]/70 font-semibold',
                            'bg-[#2563eb]/8',
                            'shadow-[0_0_0_1px_rgba(37,99,235,0.2)]',
                            'text-[#1d4ed8]',
                          ].join(' ')
                        : 'border-border bg-background text-foreground hover:border-[#2563eb]/35 hover:bg-[#2563eb]/[0.04]',
                    ].join(' ')}
                  >
                    {/* Checkmark badge — visible only when selected */}
                    <span
                      aria-hidden="true"
                      className={[
                        'absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center',
                        'bg-[#2563eb]',
                        'transition-all duration-200 ease-out',
                        statementPref === 'manual' ? 'opacity-100 scale-100' : 'opacity-0 scale-0',
                      ].join(' ')}
                    >
                      <Check className="w-3 h-3 text-white" strokeWidth={3} />
                    </span>
                    <ShieldCheck
                      className={['w-6 h-6 mb-2 transition-colors duration-200', statementPref === 'manual' ? 'text-[#2563eb]' : 'text-muted-foreground'].join(' ')}
                      aria-hidden="true"
                    />
                    <span className="font-medium">Manual</span>
                    <span className={['text-xs mt-0.5 transition-colors', statementPref === 'manual' ? 'text-[#1d4ed8]' : 'text-muted-foreground'].join(' ')}>
                      Upload PDFs yourself
                    </span>
                  </button>
                </div>
              </div>
              <div className="space-y-2">
                <Label htmlFor="llm">Local LLM Model</Label>
                <Select value={llmConfig} onValueChange={setLlmConfig}>
                  <SelectTrigger id="llm" aria-label="Select local LLM model">
                    <SelectValue placeholder="Select Model" />
                  </SelectTrigger>
                  <SelectContent>
                    {availableModels.map((m) => (
                      <SelectItem key={m.id} value={m.id}>
                        {m.name} ({m.min_ram_gb}GB+ RAM)
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="space-y-4 animate-in fade-in slide-in-from-bottom-4">
              <div className="space-y-2">
                <Label htmlFor="scan">Historical Scan Range</Label>
                <Select value={scanRange} onValueChange={setScanRange}>
                  <SelectTrigger id="scan" aria-label="Select historical scan range">
                    <SelectValue placeholder="Select months" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="1">1 Month</SelectItem>
                    <SelectItem value="3">3 Months</SelectItem>
                    <SelectItem value="6">6 Months</SelectItem>
                    <SelectItem value="12">1 Year</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-6 animate-in fade-in slide-in-from-bottom-4 text-center">
              <div className="mx-auto w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
                <ShieldCheck className="w-6 h-6 text-primary" aria-hidden="true" />
              </div>
              <div>
                <h3 className="text-lg font-medium">Connect your Gmail</h3>
                <p className="text-sm text-muted-foreground mt-2">
                  <span>We require read-only access to parse financial emails.</span>
                  Your credentials are never stored.
                </p>
              </div>
              <div className="bg-secondary/50 p-4 rounded-md text-left" aria-label="Requested Gmail scopes">
                <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Requested Scopes</span>
                <ul className="mt-2 text-sm space-y-1" >
                  <li>
                    •{' '}
                    <code className="text-xs bg-muted px-1 rounded">
                      https://www.googleapis.com/auth/gmail.readonly
                    </code>
                  </li>
                </ul>
              </div>

              {/* F12 fix: beta/"Testing" OAuth mode constraints, surfaced
                  before Gmail authorization rather than only in a separate
                  onboarding guide doc. */}
              <div
                className="bg-amber-500/10 border border-amber-500/30 p-4 rounded-md text-left"
                aria-label="Beta program disclosure"
              >
                <span className="text-xs font-semibold uppercase tracking-wider text-amber-700">
                  Beta Program — Google Sign-In Limitations
                </span>
                <ul className="mt-2 text-xs space-y-1.5 text-muted-foreground" style={{ listStyleType: 'disc', paddingLeft: '16px' }}>
                  {BETA_PROGRAM_DISCLOSURE.map((item, i) => (
                    <li key={i}>{item}</li>
                  ))}
                </ul>
              </div>

              {/* Doc 01 §10.4: presented verbatim on the onboarding consent
                  screen, before Gmail authorization. */}
              <div className="bg-secondary/50 p-4 rounded-md text-left" aria-label="Outbound network channels disclosure">
                <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  Outbound Channels Disclosure
                </span>
                <ul className="mt-2 text-xs space-y-1 text-muted-foreground" style={{ listStyleType: 'disc', paddingLeft: '16px' }}>
                  {OUTBOUND_CHANNEL_DISCLOSURE.map((item, i) => (
                    <li key={i}>{item}</li>
                  ))}
                </ul>
              </div>

              {oauthError && (
                <div
                  role="alert"
                  className="flex items-center gap-2 text-red-700 bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2 text-sm"
                >
                  <AlertCircle className="w-4 h-4 shrink-0" aria-hidden="true" />
                  {oauthError}
                </div>
              )}

              {/* G2 fix: statement-only users previously had no way to finish
                  onboarding without connecting Gmail. */}
              {statementPref === 'manual' && (
                <button
                  type="button"
                  onClick={handleSkipGmail}
                  disabled={loading}
                  className="text-sm text-muted-foreground underline underline-offset-2 hover:text-foreground disabled:opacity-50"
                >
                  Skip — I'll upload statements manually
                </button>
              )}
            </div>
          )}
        </CardContent>

        <CardFooter className="flex justify-between">
          {step > 1 ? (
            <Button variant="outline" onClick={handleBack} disabled={loading} aria-label="Go back to previous step">
              Back
            </Button>
          ) : (
            <div aria-hidden="true" /> // Spacer
          )}

          {step < TOTAL_STEPS ? (
            <Button onClick={handleNext} variant="accent" aria-label={`Continue to step ${step + 1}`}>
              Continue
            </Button>
          ) : (
            <Button onClick={handleConnectGmail} disabled={loading} variant="accent" className="gap-2" aria-label="I Understand, Continue to Google">
              {loading ? <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" /> : <Mail className="w-4 h-4" aria-hidden="true" />}
              I Understand, Continue to Google
            </Button>
          )}
        </CardFooter>
      </Card>
    </div>
  );
}
