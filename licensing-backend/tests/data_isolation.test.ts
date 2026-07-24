// Doc 30 TASK-LIC-010: continuously-run compliance suite enforcing the hard
// scope boundary -- no financial data anywhere in this codebase's request/
// response schemas, database, or outbound calls. A failing test here blocks
// deployment with the same severity as a security vulnerability.
import { describe, it, expect } from 'vitest';
import { getDMMF } from '@prisma/internals';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const REPO_ROOT = join(__dirname, '..');

function listFiles(dir: string, filter: (name: string) => boolean, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === 'node_modules' || entry === '.git' || entry === 'dist') continue;
    const full = join(dir, entry);
    const stat = statSync(full);
    if (stat.isDirectory()) listFiles(full, filter, acc);
    else if (filter(entry)) acc.push(full);
  }
  return acc;
}

const FINANCIAL_FIELD_PATTERNS = [
  /merchant/i,
  /\btransaction_amount\b/i,
  /\baccount_number\b/i,
  /\bcard_number\b/i,
  /\biban\b/i,
  /\bcvv\b/i,
  /gmail_body/i,
  /email_content/i,
  /email_body/i,
];

describe('test_no_endpoint_accepts_financial_data_fields', () => {
  it('no api/ route file declares an Input type with a financial-shaped field', () => {
    const apiFiles = listFiles(join(REPO_ROOT, 'api'), (n) => n.endsWith('.ts'));
    const violations: string[] = [];
    for (const file of apiFiles) {
      const src = readFileSync(file, 'utf-8');
      const inputBlockMatch = src.match(/export interface \w*Input\b[^{]*\{([^}]*)\}/g);
      for (const block of inputBlockMatch ?? []) {
        for (const pattern of FINANCIAL_FIELD_PATTERNS) {
          if (pattern.test(block)) violations.push(`${file}: matched ${pattern}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

describe('test_no_endpoint_returns_financial_data_fields', () => {
  it('no api/ route file declares a Result type with a financial-shaped field', () => {
    const apiFiles = listFiles(join(REPO_ROOT, 'api'), (n) => n.endsWith('.ts'));
    const violations: string[] = [];
    for (const file of apiFiles) {
      const src = readFileSync(file, 'utf-8');
      const resultBlockMatch = src.match(/export interface \w*Result\b[^{]*\{([^}]*)\}/g);
      for (const block of resultBlockMatch ?? []) {
        for (const pattern of FINANCIAL_FIELD_PATTERNS) {
          if (pattern.test(block)) violations.push(`${file}: matched ${pattern}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

describe('test_database_contains_zero_financial_tables', () => {
  it('no Prisma model name resembles a financial-data table', async () => {
    const datamodel = readFileSync(join(REPO_ROOT, 'prisma', 'schema.prisma'), 'utf-8');
    const dmmf = await getDMMF({ datamodel });
    const FINANCIAL_TABLE_PATTERNS = [
      /transaction/i,
      /statement/i,
      /merchant/i,
      /instrument/i,
      /bank/i,
    ];
    const violations = dmmf.datamodel.models
      .map((m) => m.name)
      .filter((name) => FINANCIAL_TABLE_PATTERNS.some((p) => p.test(name)));
    expect(violations).toEqual([]);
  });
});

describe('test_no_outbound_calls_to_gmail_api_or_google_oauth', () => {
  it('no source file imports or references Gmail API / Google OAuth endpoints', () => {
    const sourceFiles = [
      ...listFiles(join(REPO_ROOT, 'api'), (n) => n.endsWith('.ts')),
      ...listFiles(join(REPO_ROOT, 'lib'), (n) => n.endsWith('.ts')),
    ];
    // Deliberately scoped to real usage (imports/URLs), not the bare word --
    // a comment explaining that a desktop-side GMAIL_* code "never applies
    // here" is compliance documentation, not a violation.
    const GOOGLE_PATTERNS = [
      /googleapis\.com/i,
      /accounts\.google\.com/i,
      /from ['"].*gmail/i,
      /require\(['"].*gmail/i,
      /google[_.]?oauth/i,
    ];
    const violations: string[] = [];
    for (const file of sourceFiles) {
      const src = readFileSync(file, 'utf-8');
      for (const line of src.split('\n')) {
        if (/^\s*(\/\/|\*|\/\*)/.test(line)) continue; // skip comment lines
        for (const pattern of GOOGLE_PATTERNS) {
          if (pattern.test(line))
            violations.push(`${file}: matched ${pattern} in "${line.trim()}"`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
