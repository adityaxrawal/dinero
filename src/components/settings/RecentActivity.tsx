/**
 * What recent scans and statement imports did, kept by the backend so it
 * survives a restart (audit F5-4; the notification feed is in-memory only).
 */
import { useQuery } from '@tanstack/react-query';
import { History } from 'lucide-react';
import SectionHeading from '@/components/settings/SectionHeading';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { cn } from '@/lib/utils';

const TONE: Record<string, string> = {
  failed: 'text-red-700',
  cancelled: 'text-amber-700',
};

export default function RecentActivity() {
  const { data: entries = [], isError, refetch } = useQuery({
    queryKey: queryKeys.activity.all(),
    queryFn: API.activity.list,
  });

  return (
    <section className="mt-10" aria-label="Recent activity">
      <SectionHeading
        icon={History}
        title="Recent activity"
        description="What your last scans and statement imports did."
      />
      {isError ? (
        <p role="alert" className="text-[12px] text-emerald-ink/70">
          Couldn't load recent activity.{' '}
          <button type="button" className="underline" onClick={() => refetch()}>
            Try again
          </button>
        </p>
      ) : entries.length === 0 ? (
        <p className="text-[12px] text-emerald-ink/60">No scans or imports yet.</p>
      ) : (
        <ul className="divide-y divide-emerald-ink/10 rounded-xl border border-emerald-ink/10">
          {entries.map((e) => (
            <li key={e.id} className="p-3 text-[12px] text-emerald-ink">
              <p className={cn('font-medium', TONE[e.outcome])}>{e.summary}</p>
              {e.created_at && (
                <p className="text-emerald-ink/50">
                  {new Date(`${e.created_at.replace(' ', 'T')}Z`).toLocaleString('en-IN')}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
