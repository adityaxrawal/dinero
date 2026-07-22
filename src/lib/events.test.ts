// Doc 30 TASK-RT-001 acceptance: test_typescript_payload_types_match_rust_structs.
// Cross-language source scan (same pragmatic approach as Settings.billing.test.ts
// and licensing-backend's data_isolation.test.ts): reads the real Rust struct
// definitions and confirms every field name they declare also appears in the
// corresponding TypeScript interface in events.ts, so the two can't silently
// drift the way ScanProgressPayload's mandate_events_found field once did
// (Doc 19 §15.1 v1.14).
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const eventsTs = readFileSync(join(__dirname, 'events.ts'), 'utf-8');
const ipcTs = readFileSync(join(__dirname, 'ipc.ts'), 'utf-8');
const scanRustSrc = readFileSync(
  join(__dirname, '../../src-tauri/src/ingestion/historical_scan.rs'),
  'utf-8',
);
const indicatorRustSrc = readFileSync(
  join(__dirname, '../../src-tauri/src/background_tasks/indicator.rs'),
  'utf-8',
);

function tsInterfaceFields(source: string, interfaceName: string): string[] {
  const match = source.match(new RegExp(`interface ${interfaceName}\\s*{([^}]*)}`, 's'));
  if (!match) throw new Error(`interface ${interfaceName} not found`);
  return [...match[1].matchAll(/(\w+)\??:/g)].map((m) => m[1]);
}

function rustStructFields(source: string, structName: string): string[] {
  const match = source.match(new RegExp(`struct ${structName}\\s*{([^}]*)}`, 's'));
  if (!match) throw new Error(`struct ${structName} not found`);
  return [...match[1].matchAll(/pub\s+(\w+):|^\s*(\w+):/gm)]
    .map((m) => m[1] || m[2])
    .filter(Boolean);
}

describe('test_typescript_payload_types_match_rust_structs', () => {
  it('ScanProgressPayload (ipc.ts) matches the real Rust ScanProgressPayload field-for-field', () => {
    const tsFields = tsInterfaceFields(ipcTs, 'ScanProgressPayload').sort();
    const rustFields = rustStructFields(scanRustSrc, 'ScanProgressPayload').sort();
    expect(tsFields).toEqual(rustFields);
  });

  it('BackgroundTaskProgressPayload (ipc.ts) matches the real Rust TaskProgress field-for-field', () => {
    const tsFields = tsInterfaceFields(ipcTs, 'BackgroundTaskProgressPayload').sort();
    const rustFields = rustStructFields(indicatorRustSrc, 'TaskProgress').sort();
    expect(tsFields).toEqual(rustFields);
  });

  it('events.ts defines every documented event payload interface not already owned by ipc.ts', () => {
    for (const name of ['TransactionCreatedPayload', 'ReconciliationClusterPayload']) {
      expect(eventsTs).toContain(`interface ${name}`);
    }
  });

  it('re-exports ScanProgressPayload/SystemWarningPayload/BackgroundTaskProgressPayload from ipc.ts rather than duplicating them', () => {
    expect(eventsTs).toMatch(/export type \{[^}]*ScanProgressPayload[^}]*\} from '@\/lib\/ipc'/);
    expect(eventsTs).toMatch(/export type \{[^}]*SystemWarningPayload[^}]*\} from '@\/lib\/ipc'/);
    expect(eventsTs).toMatch(/export type \{[^}]*BackgroundTaskProgressPayload[^}]*\} from '@\/lib\/ipc'/);
  });
});
