/**
 * Portable backup: export the database protected by a password the user
 * chooses, and restore it on this or any other Mac.
 *
 * The live database key is bound to this machine's hardware, so this is the
 * only way to move data to a new machine.
 */
import { useState } from 'react';
import { Download, Upload } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';

export const MIN_BACKUP_PASSWORD_LENGTH = 8;
const FILE_FILTERS = [{ name: 'Dinero backup', extensions: ['dinero'] }];

/** Portable backup export and restore. */
export default function PortableBackupSettings() {
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const passwordError =
    password.length < MIN_BACKUP_PASSWORD_LENGTH
      ? `Password must be at least ${MIN_BACKUP_PASSWORD_LENGTH} characters.`
      : null;
  const mismatch = confirmPassword !== password;

  /** Asks where to save, then writes the password-protected backup. */
  const handleExport = async () => {
    if (passwordError || mismatch) return;
    const { save } = await import('@tauri-apps/plugin-dialog');
    const path = await save({ defaultPath: 'Dinero Backup.dinero', filters: FILE_FILTERS });
    if (!path) return;
    setBusy(true);
    setMessage(null);
    try {
      await API.db.exportBackup(path, password);
      setMessage('Backup saved. Keep the password safe: without it the backup cannot be opened.');
      setPassword('');
      setConfirmPassword('');
    } catch (err) {
      setMessage('Export failed: ' + getErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  /** Picks a backup file, confirms, and replaces this device's data with it. */
  const handleRestore = async () => {
    if (passwordError) return;
    const { open, ask } = await import('@tauri-apps/plugin-dialog');
    const path = await open({ multiple: false, directory: false, filters: FILE_FILTERS });
    if (typeof path !== 'string') return;
    const confirmed = await ask(
      'Restoring replaces all data on this device with the backup. Your current data is saved as a local backup first. Dinero will restart. Continue?',
      { title: 'Restore from backup', kind: 'warning' }
    );
    if (!confirmed) return;
    setBusy(true);
    setMessage(null);
    try {
      await API.db.restorePortableBackup(path, password);
    } catch (err) {
      setMessage('Restore failed: ' + getErrorMessage(err));
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <h3 className="text-[15px] font-bold text-[#064E3B]">Portable backup</h3>
      <p className="text-[13px] font-medium text-[#064E3B]/70">
        Export your data protected by a password you choose. The file can be restored on any Mac. To
        restore, enter the password the backup was made with.
      </p>
      <div className="grid gap-3 sm:grid-cols-2 max-w-xl">
        <div className="space-y-1">
          <Label htmlFor="backup-password" className="text-[#064E3B]">
            Backup password
          </Label>
          <Input
            id="backup-password"
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        <div className="space-y-1">
          <Label htmlFor="backup-password-confirm" className="text-[#064E3B]">
            Confirm password (export only)
          </Label>
          <Input
            id="backup-password-confirm"
            type="password"
            autoComplete="new-password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
          />
        </div>
      </div>
      {password.length > 0 && passwordError && (
        <p className="text-xs text-red-700">{passwordError}</p>
      )}
      {confirmPassword.length > 0 && mismatch && (
        <p className="text-xs text-red-700">Passwords do not match.</p>
      )}
      <div className="flex gap-3">
        <button
          type="button"
          className="h-9 px-4 rounded-lg font-semibold bg-[#064E3B] text-[#F8E7C9] disabled:opacity-50 inline-flex items-center gap-2"
          onClick={handleExport}
          disabled={busy || !!passwordError || mismatch}
        >
          <Download className="w-4 h-4" aria-hidden="true" />
          Export backup
        </button>
        <button
          type="button"
          className="h-9 px-4 rounded-lg font-semibold border border-[#064E3B]/20 text-[#064E3B] disabled:opacity-50 inline-flex items-center gap-2"
          onClick={handleRestore}
          disabled={busy || !!passwordError}
        >
          <Upload className="w-4 h-4" aria-hidden="true" />
          Restore from file
        </button>
      </div>
      {message && (
        <p role="status" className="text-[13px] text-[#064E3B]">
          {message}
        </p>
      )}
    </div>
  );
}
