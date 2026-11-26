/**
 * Connected-accounts section of settings.
 */
import ConnectedAccountsSettings from '@/components/settings/ConnectedAccountsSettings';
import MailScanSection from './MailScanSection';
import SkippedEmailsSettings from '@/components/settings/SkippedEmailsSettings';
import RecentActivity from '@/components/settings/RecentActivity';

/** Connected-accounts section. */
export default function AccountsSection() {
  return (
    <div className="animate-in fade-in duration-300 space-y-12">
      <section>
        <ConnectedAccountsSettings />
      </section>

      <div className="h-px w-full bg-emerald-ink/10" />

      <MailScanSection />
      <RecentActivity />
      <SkippedEmailsSettings />
    </div>
  );
}
