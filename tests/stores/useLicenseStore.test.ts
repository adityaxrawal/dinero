import { describe, it, expect, beforeEach } from 'vitest';
import { useLicenseStore } from '@/stores/useLicenseStore';
import type { LicenseStatusResponse } from '@/lib/ipc';

describe('useLicenseStore', () => {
  beforeEach(() => {
    useLicenseStore.setState({
      state: 'ANONYMOUS_EVAL',
      isLocked: false,
      daysRemainingInTrial: null,
      planId: null,
      billingInterval: null,
      expiryDate: null,
      hydrated: false,
    });
  });

  it('derives isLocked from is_active=false', () => {
    const status: LicenseStatusResponse = {
      state: 'LOCKED',
      is_active: false,
      license_key_masked: null,
      plan_id: null,
      billing_interval: null,
      expiry_date: null,
      days_remaining: null,
    };
    useLicenseStore.getState().applyStatus(status);
    expect(useLicenseStore.getState().isLocked).toBe(true);
    expect(useLicenseStore.getState().state).toBe('LOCKED');
    expect(useLicenseStore.getState().hydrated).toBe(true);
  });

  it('derives isLocked=false while trial days remain', () => {
    const status: LicenseStatusResponse = {
      state: 'TRIAL',
      is_active: true,
      license_key_masked: null,
      plan_id: null,
      billing_interval: null,
      expiry_date: '2026-08-01T00:00:00Z',
      days_remaining: 5,
    };
    useLicenseStore.getState().applyStatus(status);
    expect(useLicenseStore.getState().isLocked).toBe(false);
    expect(useLicenseStore.getState().daysRemainingInTrial).toBe(5);
  });

  it('a later GRACE snapshot overwrites an earlier ACTIVE one (mirrors most recent broadcast)', () => {
    useLicenseStore.getState().applyStatus({
      state: 'ACTIVE',
      is_active: true,
      license_key_masked: null,
      plan_id: 'pro',
      billing_interval: 'monthly',
      expiry_date: null,
      days_remaining: null,
    });
    expect(useLicenseStore.getState().isLocked).toBe(false);

    useLicenseStore.getState().applyStatus({
      state: 'GRACE',
      is_active: true,
      license_key_masked: null,
      plan_id: 'pro',
      billing_interval: 'monthly',
      expiry_date: null,
      days_remaining: null,
    });
    expect(useLicenseStore.getState().state).toBe('GRACE');
    expect(useLicenseStore.getState().isLocked).toBe(false);
  });
});
