/**
 * Maps a support case to an ordered recovery plan.
 *
 * The organising idea is escalating invasiveness: a refresh costs the user
 * nothing, whereas a full local reset destroys their local data. Steps are
 * therefore always attempted least-destructive first, and INVASIVENESS_ORDER is
 * what encodes that ranking so a plan can never be assembled out of order.
 */
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

/**
 * Returns an ordered recovery plan for a support case.
 *
 * Ordered least-destructive first: a refresh costs the user nothing, whereas a
 * full local reset destroys their local data.
 */
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
