/**
 * Manages saved statement-PDF passwords.
 *
 * Shows which instruments have a stored password and allows forgetting one. The
 * secrets live in the OS keychain and are never displayed here.
 */
import { useState, useEffect } from 'react';
import { Lock, Trash2 } from 'lucide-react';
import { API, PdfPasswordSummary } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';

/** Manages saved statement-PDF passwords. */
export default function StatementPasswordSettings() {
  const [pdfPasswords, setPdfPasswords] = useState<PdfPasswordSummary[]>([]);
  const [isLoadingPasswords, setIsLoadingPasswords] = useState(true);
  const [deletingPasswordId, setDeletingPasswordId] = useState<string | null>(null);

  /** Loads saved password metadata. */
  const loadPdfPasswords = async () => {
    setIsLoadingPasswords(true);
    try {
      const passwords = await API.pdfPasswords.list();
      setPdfPasswords(passwords);
    } catch (err) {
      console.error('Failed to fetch stored PDF passwords:', err);
    } finally {
      setIsLoadingPasswords(false);
    }
  };

  useEffect(() => {
    loadPdfPasswords();
  }, []);

  /** Forgets a saved password, removing it from the keychain. */
  const handleDeletePassword = async (password: PdfPasswordSummary) => {
    let confirmed: boolean;
    const warning = `Forget the stored password for ${password.issuer_name} •••• ${password.masked_identifier}? You'll be prompted again next time a statement from this account needs unlocking.`;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Forget Password', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setDeletingPasswordId(password.id);
    try {
      await API.pdfPasswords.delete(password.id);
      await loadPdfPasswords();
    } catch (err) {
      alert('Failed to delete password: ' + getErrorMessage(err));
    } finally {
      setDeletingPasswordId(null);
    }
  };

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 mb-4">
        <Lock className="w-5 h-5 text-[#064E3B]" />
        <h3 className="text-xl font-bold text-[#064E3B]">Stored Statement Passwords</h3>
      </div>
      <p className="text-sm text-[#064E3B]/70 mb-4">
        Passwords Dinero has learned for encrypted statements, encrypted at rest and never shown
        here. Forgetting one just means you'll be re-prompted next time.
      </p>

      {isLoadingPasswords ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">Loading…</p>
      ) : pdfPasswords.length === 0 ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">No stored passwords yet.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {pdfPasswords.map((pw) => (
            <div
              key={pw.id}
              className="flex items-center justify-between gap-4 p-4 rounded-xl border border-[#064E3B]/10 bg-[#064E3B]/5"
            >
              <div>
                <strong className="text-[14px] font-bold text-[#064E3B]">{pw.issuer_name}</strong>
                <span className="text-[13px] font-medium text-[#064E3B]/70">
                  {' '}
                  •••• {pw.masked_identifier}
                </span>
                <div className="text-[12px] font-medium text-[#064E3B]/60 mt-1">
                  Used successfully {pw.success_count} time{pw.success_count === 1 ? '' : 's'}
                  {pw.last_used_at
                    ? ` — last on ${new Date(pw.last_used_at).toLocaleDateString()}`
                    : ''}
                </div>
              </div>
              <button
                className="h-8 px-3 text-[12px] font-semibold rounded-lg border border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 hover:border-red-300 transition-colors flex items-center gap-1.5 disabled:opacity-50"
                onClick={() => handleDeletePassword(pw)}
                disabled={deletingPasswordId === pw.id}
              >
                <Trash2 className="w-3.5 h-3.5" />{' '}
                {deletingPasswordId === pw.id ? 'Forgetting…' : 'Forget'}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
