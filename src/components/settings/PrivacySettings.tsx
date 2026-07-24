import { useState, useEffect } from 'react';
import { History, ShieldAlert, FileText } from 'lucide-react';
import { API, ConsentEventRecord } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';
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
    <div className="space-y-12">
      <div className="space-y-5">
        <div className="flex items-center gap-2 mb-4">
          <History className="w-5 h-5 text-[#064E3B]" />
          <h3 className="text-xl font-bold text-[#064E3B]">Consent History</h3>
        </div>
        <ConsentHistoryList
          events={consentHistory}
          isLoading={isLoadingConsent}
          onRefresh={loadConsentHistory}
        />
      </div>

      <div className="h-px w-full bg-[#064E3B]/10" />

      <div className="space-y-5">
        <div className="flex items-center gap-2 mb-4">
          <ShieldAlert className="w-5 h-5 text-red-600" />
          <h3 className="text-xl font-bold text-red-600">Privacy &amp; Data</h3>
        </div>
        <p className="text-sm text-[#064E3B]/70 mb-5">
          Your data is encrypted and stored locally on this device.
        </p>

        <div className="mb-6 pb-6 border-b border-[#064E3B]/10">
          <p className="text-sm text-[#064E3B]/70 mb-4">
            A diagnostic bundle (app logs and error reports) helps troubleshoot issues. It never
            includes your financial data, and it is{' '}
            <strong>saved locally on this device only</strong> — it is never automatically uploaded
            anywhere. You choose if and when to share it.
          </p>
          <button
            className="h-9 px-4 rounded-lg font-semibold bg-[#064E3B]/5 border border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors inline-flex items-center justify-center gap-2 disabled:opacity-50"
            onClick={handleExportDiagnosticBundle}
            disabled={isExporting}
          >
            <FileText className="w-4 h-4" />
            {isExporting ? 'Exporting…' : 'Export Diagnostic Bundle'}
          </button>
          {exportedPath && (
            <p className="text-[12px] font-medium text-[#064E3B]/70 mt-3">
              Saved locally to: {exportedPath}
            </p>
          )}
        </div>

        <DeleteAccountSection />
      </div>
    </div>
  );
}
