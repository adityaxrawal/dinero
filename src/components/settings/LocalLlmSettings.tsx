/**
 * Local LLM configuration: model selection, download, and concurrency.
 *
 * Surfaces the hardware constraints the backend computed, so the choice is made
 * against what this machine can actually run rather than against a wish.
 */
import { Cpu } from 'lucide-react';
import SectionHeading from './SectionHeading';
import { useLlmModels } from './localLlm/useLlmModels';
import { useParallelSlots } from './localLlm/useParallelSlots';
import ParallelSlotsCard from './localLlm/ParallelSlotsCard';
import HeavyModelNotice from './localLlm/HeavyModelNotice';
import ModelCard from './localLlm/ModelCard';

const BLURB =
  'Select the local AI model for parsing statements. Heavier models perform better but require more RAM. Models must be downloaded before they can be selected.';

/** Local LLM configuration: model, download, concurrency. */
export default function LocalLlmSettings() {
  const slots = useParallelSlots();
  const models = useLlmModels((hw) => slots.adoptDefault(hw.recommended_slots));

  return (
    <section>
      <SectionHeading icon={Cpu} title="Local LLM Configuration" description={BLURB} />

      <ParallelSlotsCard slots={slots} hwInfo={models.hwInfo} />

      <HeavyModelNotice
        hwInfo={models.hwInfo}
        availableModels={models.availableModels}
        activeModel={models.activeModel}
      />

      <div className="grid grid-cols-1 gap-4">
        {models.availableModels.map((m) => (
          <ModelCard key={m.id} model={m} models={models} />
        ))}
      </div>
    </section>
  );
}
