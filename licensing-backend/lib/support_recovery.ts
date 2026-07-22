// Doc 30 TASK-OPS-006, Doc 43 §3 action 7: "a scripted recovery flow for
// legitimate cases ... self-describing so the operator chooses the least
// invasive recovery first (refresh, rebind, restore backup, full local
// reset)." A pure decision table so the ordering is independently testable
// (`test_support_flow_uses_least_invasive_recovery_first`) rather than left
// implicit in a runbook document only a human reads.

export type SupportCaseType =
  | 'stuck_grace_or_locked'
  | 'lost_or_replaced_device'
  | 'reinstalled_os'
  | 'corrupted_local_state';

export type RecoveryAction = 'refresh' | 'rebind' | 'restore_backup' | 'full_local_reset';

export interface RecoveryStep {
  action: RecoveryAction;
  description: string;
}

/// Doc 30's own explicit invasiveness ordering. Every case's recommended
/// steps must appear in this relative order (a later-in-this-array action
/// must never be recommended before an earlier one that also applies).
export const INVASIVENESS_ORDER: RecoveryAction[] = [
  'refresh',
  'rebind',
  'restore_backup',
  'full_local_reset',
];

const STEP: Record<RecoveryAction, RecoveryStep> = {
  refresh: {
    action: 'refresh',
    description:
      'Ask the user to click "Refresh License" in Settings -- resolves a stuck GRACE/LOCKED state caused by a missed webhook with no admin action at all.',
  },
  rebind: {
    action: 'rebind',
    description:
      'Admin resets the device binding (support_reset_binding) so the user can reactivate from their current or a new Mac.',
  },
  restore_backup: {
    action: 'restore_backup',
    description:
      'User restores their local daily/manual encrypted backup -- recovers local data without touching the Licensing Backend at all.',
  },
  full_local_reset: {
    action: 'full_local_reset',
    description:
      'User resets local app data entirely and reactivates from scratch -- last resort, discards all local data.',
  },
};

/// Recommends the ordered recovery steps for a support case, least invasive
/// first. Only the steps that actually apply to the case are included --
/// e.g. a corrupted local database has nothing to do with device binding,
/// so `refresh`/`rebind` are never recommended for it.
export function recommendRecovery(caseType: SupportCaseType): RecoveryStep[] {
  switch (caseType) {
    case 'stuck_grace_or_locked':
      return [STEP.refresh, STEP.rebind];
    case 'reinstalled_os':
      return [STEP.refresh, STEP.rebind];
    case 'lost_or_replaced_device':
      return [STEP.rebind];
    case 'corrupted_local_state':
      return [STEP.restore_backup, STEP.full_local_reset];
  }
}
