import PrivacySettings from '@/components/settings/PrivacySettings';
import StatementPasswordSettings from '@/components/settings/StatementPasswordSettings';
import LifecycleSettings from '@/components/settings/LifecycleSettings';
import RecoveryPhraseSection from './RecoveryPhraseSection';

export default function PrivacySection() {
  return (
    <div className="animate-in fade-in duration-300 space-y-12">
      <section>
        <PrivacySettings />
      </section>
      <div className="h-px w-full bg-[#064E3B]/10" />
      <section>
        <StatementPasswordSettings />
      </section>
      <div className="h-px w-full bg-[#064E3B]/10" />
      <RecoveryPhraseSection />
      <div className="h-px w-full bg-[#064E3B]/10" />
      <section>
        <LifecycleSettings />
      </section>
    </div>
  );
}
