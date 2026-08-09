import ConnectedAccountsSettings from '@/components/settings/ConnectedAccountsSettings';
import MailScanSection from './MailScanSection';

export default function AccountsSection() {
  return (
    <div className="animate-in fade-in duration-300 space-y-12">
      <section>
        <ConnectedAccountsSettings />
      </section>

      <div className="h-px w-full bg-[#064E3B]/10" />

      <MailScanSection />
    </div>
  );
}
