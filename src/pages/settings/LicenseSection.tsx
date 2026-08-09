import { CreditCard, Loader2 } from 'lucide-react';
import SectionHeading from '@/components/settings/SectionHeading';
import { useLicenseStatus } from './useLicenseStatus';
import { useLicenseActivation } from './useLicenseActivation';
import LicenseStateBanner from './LicenseStateBanner';
import LicenseStatusGrid from './LicenseStatusGrid';
import LicenseActions from './LicenseActions';
import ManualActivationForm from './ManualActivationForm';

export default function LicenseSection() {
  const license = useLicenseStatus();
  const activation = useLicenseActivation({
    reload: license.loadLicenseStatus,
    setError: license.setLicenseActionError,
  });
  const { licenseStatus } = license;

  return (
    <div className="animate-in fade-in duration-300 space-y-6">
      <SectionHeading icon={CreditCard} title="License & Billing" />

      {license.isLoadingLicense ? (
        <div className="py-8 flex justify-center">
          <Loader2 className="w-5 h-5 animate-spin text-[#064E3B]/50" />
        </div>
      ) : licenseStatus ? (
        <div className="space-y-6">
          <LicenseStateBanner status={licenseStatus} />
          <LicenseStatusGrid status={licenseStatus} />

          {license.licenseActionError && (
            <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 text-[13px] font-medium text-red-700">
              {license.licenseActionError}
            </div>
          )}

          <LicenseActions status={licenseStatus} license={license} activation={activation} />

          {activation.showActivateForm && !licenseStatus.is_active && (
            <ManualActivationForm activation={activation} />
          )}
        </div>
      ) : (
        <p className="text-[13px] font-medium text-[#064E3B]/70">Could not load license status.</p>
      )}
    </div>
  );
}
