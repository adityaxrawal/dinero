import { useState, useEffect } from 'react';
import { Lock, Trash2 } from 'lucide-react';
import { API, PdfPasswordSummary } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/getErrorMessage';

/**
 * TASK-FE-015 (Doc 30): "instruments with a saved-password indicator
 * (never the password) plus a 'Forget Password' action." Extracted
 * verbatim from the pre-existing "Stored Statement Passwords" section
 * (G15 fix) -- already instrument-scoped (issuer_name/masked_identifier
 * are the instrument's own identity fields) and already never transmits
 * the password itself (`PdfPasswordSummary` has no password field at all,
 * by construction on the backend).
 */
export default function StatementPasswordSettings() {
  const [pdfPasswords, setPdfPasswords] = useState<PdfPasswordSummary[]>([]);
  const [isLoadingPasswords, setIsLoadingPasswords] = useState(true);
  const [deletingPasswordId, setDeletingPasswordId] = useState<string | null>(null);

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
    <div className="glass-panel" style={{ padding: '24px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '16px' }}>
        <Lock className="text-accent" size={24} color="var(--accent-primary)" />
        <h3 className="heading-md">Stored Statement Passwords</h3>
      </div>
      <p className="text-sm text-muted" style={{ marginBottom: '16px' }}>
        Passwords Dinero has learned for encrypted statements, encrypted at rest and never shown here.
        Forgetting one just means you'll be re-prompted next time.
      </p>

      {isLoadingPasswords ? (
        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Loading…</p>
      ) : pdfPasswords.length === 0 ? (
        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>No stored passwords yet.</p>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
          {pdfPasswords.map((pw) => (
            <div
              key={pw.id}
              style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px',
                padding: '10px 14px', borderRadius: '8px',
                background: 'var(--bg-secondary)', border: '1px solid var(--border)', fontSize: '12px',
              }}
            >
              <div>
                <strong style={{ color: 'var(--text-primary)' }}>{pw.issuer_name}</strong>
                <span style={{ color: 'var(--text-muted)' }}> •••• {pw.masked_identifier}</span>
                <div style={{ color: 'var(--text-muted)', marginTop: '2px' }}>
                  Used successfully {pw.success_count} time{pw.success_count === 1 ? '' : 's'}
                  {pw.last_used_at ? ` — last on ${new Date(pw.last_used_at).toLocaleDateString()}` : ''}
                </div>
              </div>
              <button
                className="btn btn-secondary"
                style={{ padding: '6px 12px', fontSize: '12px', color: '#ef4444', display: 'flex', alignItems: 'center', gap: '6px' }}
                onClick={() => handleDeletePassword(pw)}
                disabled={deletingPasswordId === pw.id}
              >
                <Trash2 size={12} /> {deletingPasswordId === pw.id ? 'Forgetting…' : 'Forget'}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
