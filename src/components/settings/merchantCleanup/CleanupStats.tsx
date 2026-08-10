/**
 * Summary statistics for a completed run.
 */
import { FileWarning, Landmark, ListChecks, Timer } from 'lucide-react';
import type { MerchantCleanupPreview, LlmModelInfo } from '@/lib/ipc';
import { StatStrip, StatTile } from '../SettingsPrimitives';
import { estimateMinutes } from './format';

/** Summary statistics for a completed run. */
export default function CleanupStats({
  preview,
  activeModel,
}: {
  preview: MerchantCleanupPreview;
  activeModel: LlmModelInfo | null;
}) {
  return (
    <div className="mb-4">
      <StatStrip>
        <StatTile
          icon={<ListChecks />}
          label="Need attention"
          value={preview.candidate_count}
          hint="scored below the threshold"
        />
        <StatTile
          icon={<FileWarning />}
          label="Will be skipped"
          value={preview.no_evidence_count}
          hint="email no longer kept"
          tone={preview.no_evidence_count > 0 ? 'warn' : 'default'}
        />
        <StatTile
          icon={<Landmark />}
          label="Banks affected"
          value={preview.by_bank.length}
          hint={preview.by_bank[0]?.bank_name}
        />
        <StatTile
          icon={<Timer />}
          label="Estimated time"
          value={estimateMinutes(preview.candidate_count - preview.no_evidence_count)}
          hint={activeModel ? activeModel.name : 'no model selected'}
        />
      </StatStrip>
    </div>
  );
}
