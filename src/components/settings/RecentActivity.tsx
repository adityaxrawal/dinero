/**
 * What recent scans, statement imports, duplicate checks and local-AI reads
 * did, kept by the backend so it survives a restart (audit F5-4, F19-3; the
 * notification feed is in-memory only).
 */
import { useQuery } from '@tanstack/react-query';
import { History } from 'lucide-react';
import SectionHeading from '@/components/settings/SectionHeading';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { cn } from '@/lib/utils';
import LoadError from '@/components/common/LoadError';

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
        description="What your recent scans, statement imports, duplicate checks and AI reads did."
      />
      {isError ? (
        <LoadError what="recent activity" onRetry={() => refetch()} />
      ) : entries.length === 0 ? (
        <p className="text-[12px] text-emerald-ink/85">Nothing has happened yet.</p>
      ) : (
        <ul className="divide-y divide-emerald-ink/10 rounded-xl border border-emerald-ink/10">
          {entries.map((e) => (
            <li key={e.id} className="p-3 text-[12px] text-emerald-ink">
              <p className={cn('font-medium', TONE[e.outcome])}>{e.summary}</p>
              {e.created_at && (
                <p className="text-emerald-ink/80">
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
