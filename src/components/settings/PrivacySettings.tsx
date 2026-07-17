import { useState, useEffect } from 'react';
import { History, ShieldAlert, FileText } from 'lucide-react';
import { API, ConsentEventRecord } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/getErrorMessage';
import ConsentHistoryList from './ConsentHistoryList';
import DeleteAccountSection from './DeleteAccountSection';

/**
 * TASK-FE-014 (Doc 30): "Build Settings — Privacy and Consent History
 * Page." Composes the always-accessible consent history, the "Export
 * Diagnostic Bundle" action (with copy explicitly stating the bundle is
 * saved locally and never auto-uploaded -- the pre-existing button had no
 * such copy at all), and the two-step delete-my-data flow.
 */
export default function PrivacySettings() {
  const [consentHistory, setConsentHistory] = useState<ConsentEventRecord[]>([]);
  const [isLoadingConsent, setIsLoadingConsent] = useState(true);
  const [isExporting, setIsExporting] = useState(false);
  const [exportedPath, setExportedPath] = useState<string | null>(null);

  const loadConsentHistory = async () => {
    setIsLoadingConsent(true);
    try {
      const events = await API.privacy.getConsentHistory();
      setConsentHistory(events);
    } catch (err) {
      console.error('Failed to fetch consent history:', err);
    } finally {
      setIsLoadingConsent(false);
    }
  };

  useEffect(() => {
    loadConsentHistory();
  }, []);

  const handleExportDiagnosticBundle = async () => {
    setIsExporting(true);
    setExportedPath(null);
    try {
      const result = await API.support.exportLogs();
      setExportedPath(result.file_path);
    } catch (err) {
      alert('Failed to export diagnostic bundle: ' + getErrorMessage(err));
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <>
      <div className="glass-panel" style={{ padding: '24px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <History className="text-accent" size={24} color="var(--accent-primary)" />
          <h3 className="heading-md">Consent History</h3>
        </div>
        <ConsentHistoryList events={consentHistory} isLoading={isLoadingConsent} onRefresh={loadConsentHistory} />
      </div>

      <div className="glass-panel" style={{ padding: '24px', borderColor: 'rgba(239, 68, 68, 0.2)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
          <ShieldAlert className="text-danger" size={24} color="#ef4444" />
          <h3 className="heading-md" style={{ color: '#ef4444' }}>Privacy &amp; Data</h3>
        </div>
        <p className="text-sm text-muted" style={{ marginBottom: '20px' }}>
          Your data is encrypted and stored locally on this device.
        </p>

        <div style={{ marginBottom: '24px', paddingBottom: '24px', borderBottom: '1px solid var(--border)' }}>
          <p className="text-sm text-muted" style={{ marginBottom: '12px' }}>
            A diagnostic bundle (app logs and error reports) helps troubleshoot issues. It never
            includes your financial data, and it is <strong>saved locally on this device only</strong> —
            it is never automatically uploaded anywhere. You choose if and when to share it.
          </p>
          <button className="btn btn-secondary" onClick={handleExportDiagnosticBundle} disabled={isExporting}>
            <FileText size={18} />
            {isExporting ? 'Exporting…' : 'Export Diagnostic Bundle'}
          </button>
          {exportedPath && (
            <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '8px' }}>
              Saved locally to: {exportedPath}
            </p>
          )}
        </div>

        <DeleteAccountSection />
      </div>
    </>
  );
}
