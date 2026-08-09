import { useState, useEffect } from 'react';
import { API, LicenseStatusResponse } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';
import { errorMessage } from '@/lib/utils';

const DEACTIVATE_WARNING =
  "Deactivate this device's license? Paid features will be unavailable here until you reactivate.";

export function useLicenseStatus() {
  const [licenseStatus, setLicenseStatus] = useState<LicenseStatusResponse | null>(null);
  const [isLoadingLicense, setIsLoadingLicense] = useState(true);
  const [isRefreshingLicense, setIsRefreshingLicense] = useState(false);
  const [isDeactivating, setIsDeactivating] = useState(false);
  const [licenseActionError, setLicenseActionError] = useState<string | null>(null);

  const loadLicenseStatus = async () => {
    setIsLoadingLicense(true);
    try {
      const status = await API.licensing.getStatus();
      setLicenseStatus(status);
    } catch {
      // Ignore initial load error
    } finally {
      setIsLoadingLicense(false);
    }
  };

  useEffect(() => {
    loadLicenseStatus();
  }, []);

  const handleRefreshLicense = async () => {
    setIsRefreshingLicense(true);
    setLicenseActionError(null);
    try {
      await API.licensing.refresh();
      await loadLicenseStatus();
    } catch (err: unknown) {
      setLicenseActionError(errorMessage(err));
    } finally {
      setIsRefreshingLicense(false);
    }
  };

  const handleDeactivateLicense = async () => {
    if (!(await confirmAction(DEACTIVATE_WARNING, 'Deactivate License'))) return;

    setIsDeactivating(true);
    setLicenseActionError(null);
    try {
      await API.licensing.deactivate();
      await loadLicenseStatus();
    } catch (err: unknown) {
      setLicenseActionError(errorMessage(err));
    } finally {
      setIsDeactivating(false);
    }
  };

  return {
    licenseStatus,
    isLoadingLicense,
    isRefreshingLicense,
    isDeactivating,
    licenseActionError,
    setLicenseActionError,
    loadLicenseStatus,
    handleRefreshLicense,
    handleDeactivateLicense,
  };
}
