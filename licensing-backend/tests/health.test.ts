// Doc 30 TASK-OPS-003 acceptance: `test_license_health_endpoint_returns_minimal_metadata`.
import { describe, it, expect, vi } from 'vitest';
import { checkHealth, type HealthDb } from '../api/health';

describe('test_license_health_endpoint_returns_minimal_metadata', () => {
  it('reports ok with only status/latency fields when the DB responds', async () => {
    const db: HealthDb = { $queryRaw: vi.fn().mockResolvedValue([{ '?column?': 1 }]) };
    const result = await checkHealth(db);
    expect(result.status).toBe('ok');
    expect(typeof result.db_latency_ms).toBe('number');
    expect(Object.keys(result).sort()).toEqual(['db_latency_ms', 'status']);
  });

  it('reports degraded, not a raw error/stack, when the DB query fails', async () => {
    const db: HealthDb = { $queryRaw: vi.fn().mockRejectedValue(new Error('connection refused')) };
    const result = await checkHealth(db);
    expect(result.status).toBe('degraded');
    expect(JSON.stringify(result)).not.toMatch(/connection refused|Error|stack/i);
  });

  it('never includes account/license/billing identity fields', async () => {
    const db: HealthDb = { $queryRaw: vi.fn().mockResolvedValue([{ '?column?': 1 }]) };
    const result = await checkHealth(db);
    const json = JSON.stringify(result).toLowerCase();
    for (const forbidden of ['email', 'license', 'device', 'token', 'account_id', 'jwt']) {
      expect(json).not.toContain(forbidden);
    }
  });
});
