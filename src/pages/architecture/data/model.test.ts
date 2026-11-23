// @vitest-environment node
/**
 * Model-vs-code verification for the architecture explorer.
 *
 * The explorer labels its components "verified"; this suite is what makes that
 * true. It fails when the hand-written model drifts from the code: a source
 * path moves, an API symbol is renamed, a column or FK changes in a migration,
 * or the enforced license transition graph changes.
 */
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';
import { describe, expect, it } from 'vitest';
import { MODEL, danglingComponentIds } from './index';

const ROOT = fileURLToPath(new URL('../../../../', import.meta.url));
const MIGRATIONS = join(ROOT, 'src-tauri/migrations');

function readTree(dir: string, ext: string): string {
  return readdirSync(dir)
    .map((name) => {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) return readTree(p, ext);
      return p.endsWith(ext) ? readFileSync(p, 'utf8') : '';
    })
    .join('\n');
}

const migrationFiles = readdirSync(MIGRATIONS)
  .filter((f) => f.endsWith('.sql'))
  .sort();
const sqlText = migrationFiles.map((f) => readFileSync(join(MIGRATIONS, f), 'utf8')).join('\n');
const rustText = readTree(join(ROOT, 'src-tauri/src'), '.rs');

/** The real schema: every migration applied in order to an in-memory DB. */
const db = new DatabaseSync(':memory:');
for (const f of migrationFiles) db.exec(readFileSync(join(MIGRATIONS, f), 'utf8'));
const columnsOf = (table: string) =>
  (db.prepare(`SELECT name FROM pragma_table_xinfo(?)`).all(table) as { name: string }[]).map(
    (r) => r.name
  );
const fkTargets = (table: string) =>
  (
    db.prepare(`SELECT "table" AS t FROM pragma_foreign_key_list(?)`).all(table) as { t: string }[]
  ).map((r) => r.t);

describe('architecture model ↔ codebase', () => {
  it('references only known components', () => {
    expect(danglingComponentIds()).toEqual([]);
  });

  it('points every component at source files that exist', () => {
    const missing = MODEL.components.flatMap((c) =>
      c.sources.filter((s) => !existsSync(join(ROOT, s.split('::')[0]))).map((s) => `${c.id}: ${s}`)
    );
    expect(missing).toEqual([]);
  });

  it('gives every workflow step component a lane', () => {
    const offLane = MODEL.workflows.flatMap((w) =>
      w.steps.flatMap((s) =>
        s.components
          .filter((id) => !w.participants.includes(id))
          .map((id) => `${w.id}/${s.id}: ${id}`)
      )
    );
    expect(offLane).toEqual([]);
  });

  it('names only API symbols that exist in the Rust code or migrations', () => {
    const corpus = `${rustText}\n${sqlText}`;
    const unknown = MODEL.components.flatMap((c) =>
      c.apiSurface
        .flatMap((entry) => entry.split(/[\s,()·/.=]+/))
        .filter((tok) => /^[a-z][a-z0-9]*(_[a-z0-9]+)+$/.test(tok))
        .filter((tok) => !new RegExp(`\\b${tok}\\b`).test(corpus))
        .map((tok) => `${c.id}: ${tok}`)
    );
    expect(unknown).toEqual([]);
  });

  it('draws ER entities and columns that exist in the migrated schema', () => {
    const problems = MODEL.entities.flatMap((e) => {
      const cols = columnsOf(e.name);
      if (cols.length === 0) return [`${e.name}: no such table`];
      return e.columns.filter((c) => !cols.includes(c.name)).map((c) => `${e.name}.${c.name}`);
    });
    expect(problems).toEqual([]);
  });

  it('draws enforced relations only where a FOREIGN KEY exists (and logical ones only where none does)', () => {
    const name = (id: string) => MODEL.entities.find((e) => e.id === id)?.name ?? id;
    const wrong = MODEL.entityRelations.flatMap((r) => {
      const hasFk = fkTargets(name(r.to)).includes(name(r.from));
      return hasFk === !r.logical ? [] : [`${r.from} → ${r.to} (${r.logical ? 'logical' : 'FK'})`];
    });
    expect(wrong).toEqual([]);
  });

  it('draws exactly the license transitions is_legal_transition enforces', () => {
    const src = readFileSync(join(ROOT, 'src-tauri/src/licensing/state_machine.rs'), 'utf8');
    const body = src.slice(src.indexOf('fn is_legal_transition'), src.indexOf('pub fn transition'));
    const enforced = [...body.matchAll(/\((\w+),\s*(\w+)\)/g)]
      .map((m) => `${m[1]}>${m[2]}`)
      .filter((p) => !p.startsWith('from>'))
      .sort();
    const license = MODEL.stateMachines.find((m) => m.id === 'license')!;
    // Server-driven moves (apply_server_validation) are drawn too, but only the
    // local edges must match is_legal_transition.
    const drawn = license.transitions
      .filter((t) => t.from !== 'start' && !t.server)
      .map((t) => `${t.from}>${t.to}`)
      .sort();
    expect(drawn).toEqual(enforced);
  });

  it('draws exactly the historical-scan statuses the code uses', () => {
    const scanSrc = readFileSync(join(ROOT, 'src-tauri/src/ingestion/historical_scan.rs'), 'utf8');
    const live = scanSrc.slice(0, scanSrc.indexOf('#[cfg(test)]'));
    const candidates = [
      'not_started',
      'in_progress',
      'paused',
      'completed',
      'completed_with_errors',
      'failed',
      'cancelled',
    ];
    const used = candidates.filter((s) => live.includes(`"${s}"`)).sort();
    const scan = MODEL.stateMachines.find((m) => m.id === 'scan')!;
    const drawn = scan.states
      .filter((s) => !s.start)
      .map((s) => s.id)
      .sort();
    expect(drawn).toEqual(used);
  });
});
