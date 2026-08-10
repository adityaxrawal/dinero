/**
 * Recovery screen shown when the database cannot be opened.
 *
 * Replaces the entire shell, because navigation is meaningless when no screen
 * behind it can load. Offers restore-from-backup or start-fresh.
 */
import { AlertTriangle, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';

/**
 * Recovery screen shown when the database cannot be opened.
 *
 * Replaces the whole shell, since navigation is meaningless when no screen
 * behind it can load.
 */
export default function CorruptedDbScreen({
  isRestoring,
  isStartingFresh,
  onRestore,
  onStartFresh,
}: {
  isRestoring: boolean;
  isStartingFresh: boolean;
  onRestore: () => void;
  onStartFresh: () => void;
}) {
  const busy = isRestoring || isStartingFresh;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="db-corrupted-title"
      aria-describedby="db-corrupted-desc"
      className="flex flex-col items-center justify-center h-screen p-8"
      style={{ backgroundColor: '#F8E7C9' }}
    >
      <div
        className="max-w-md w-full rounded-2xl p-8 flex flex-col items-center text-center gap-5"
        style={{
          backgroundColor: 'hsl(38, 55%, 91%)',
          border: '1px solid #d9c8a8',
          boxShadow: '0 4px 24px rgba(6,78,59,0.10)',
        }}
      >
        <div
          className="h-14 w-14 rounded-2xl flex items-center justify-center"
          style={{ backgroundColor: 'rgba(239,68,68,0.10)' }}
        >
          <AlertTriangle className="w-7 h-7" style={{ color: '#dc2626' }} aria-hidden="true" />
        </div>
        <div>
          <h2
            id="db-corrupted-title"
            className="text-xl font-semibold mb-2"
            style={{ color: '#0d2b22' }}
          >
            Database Corrupted
          </h2>
          <p id="db-corrupted-desc" className="text-sm" style={{ color: '#3d5a50' }}>
            The SQLite integrity check failed. Restore from a backup to recover your data, or start
            fresh.
          </p>
        </div>
        <div className="flex flex-col gap-3 w-full">
          <Button
            onClick={onRestore}
            disabled={busy}
            aria-label="Restore database from backup"
            className="w-full font-semibold"
            style={{ backgroundColor: '#064E3B', color: '#F8E7C9' }}
          >
            {isRestoring ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Restoring…
              </>
            ) : (
              'Restore from Backup'
            )}
          </Button>
          <Button
            variant="outline"
            onClick={onStartFresh}
            disabled={busy}
            aria-label="Delete all local data and start fresh"
            className="w-full"
            style={{ borderColor: '#d9c8a8', color: '#3d5a50' }}
          >
            {isStartingFresh ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Starting Fresh…
              </>
            ) : (
              'Start Fresh (Delete All Data)'
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
