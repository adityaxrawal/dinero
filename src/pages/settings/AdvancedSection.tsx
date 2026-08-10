/**
 * Advanced settings, including destructive actions.
 */
import NetworkActivity from '@/components/NetworkActivity';
import LocalLlmSettings from '@/components/settings/LocalLlmSettings';
import MerchantCleanupSettings from '@/components/settings/MerchantCleanupSettings';
import LearnedRulesSettings from '@/components/settings/LearnedRulesSettings';

/** Advanced settings, including destructive actions. */
export default function AdvancedSection() {
  return (
    <div className="animate-in fade-in duration-300 space-y-12">
      <LocalLlmSettings />

      <div className="h-px w-full bg-[#064E3B]/10" />

      <MerchantCleanupSettings />

      <div className="h-px w-full bg-[#064E3B]/10" />

      <LearnedRulesSettings />

      <div className="h-px w-full bg-[#064E3B]/10" />

      <section>
        <NetworkActivity />
      </section>
    </div>
  );
}
