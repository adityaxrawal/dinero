import { Loader2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { LicenseStatusResponse } from '@/lib/ipc';
import type { useLicenseStatus } from './useLicenseStatus';
import type { useLicenseActivation } from './useLicenseActivation';
import LicenseCtaButton from './LicenseCtaButton';

/// Doc 30 TASK-BILL-010: "Manage billing" delegates to Razorpay's hosted
/// customer portal, never rendered inside this app.
const handleManageBilling = async () => {
  const { openUrl } = await import('@tauri-apps/plugin-opener');
  await openUrl('https://dashboard.razorpay.com/customer-portal');
};

export default function LicenseActions({
  status,
  license,
  activation,
}: {
  status: LicenseStatusResponse;
  license: ReturnType<typeof useLicenseStatus>;
  activation: ReturnType<typeof useLicenseActivation>;
}) {
  const { isRefreshingLicense, handleRefreshLicense, isDeactivating, handleDeactivateLicense } =
    license;

  return (
    <div className="flex flex-wrap gap-3">
      <Button
        variant="outline"
        className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
        onClick={handleRefreshLicense}
        disabled={isRefreshingLicense}
      >
        {isRefreshingLicense ? (
          <Loader2 className="w-4 h-4 mr-2 animate-spin" />
        ) : (
          <RefreshCw className="w-4 h-4 mr-2" />
        )}{' '}
        Refresh License
      </Button>

      <LicenseCtaButton
        status={status}
        isCheckingOut={activation.isCheckingOut}
        onManageBilling={handleManageBilling}
        onSubscribe={activation.handleSubscribeNow}
      />

      {status.is_active && (
        <Button
          variant="outline"
          className="h-9 font-semibold border-red-200 text-red-600 hover:text-red-700 hover:bg-red-50 hover:border-red-300"
          onClick={handleDeactivateLicense}
          disabled={isDeactivating}
        >
          {isDeactivating ? 'Deactivating…' : 'Deactivate License'}
        </Button>
      )}

      <Button
        variant="outline"
        className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B]/70 hover:bg-[#064E3B]/5"
        onClick={() => activation.setShowActivateForm((v) => !v)}
      >
        {activation.showActivateForm
          ? 'Cancel manual entry'
          : 'Enter payment confirmation manually'}
      </Button>
    </div>
  );
}
