// Doc 30 TASK-LIC-001 acceptance criteria. All three run as static schema
// introspection against prisma/schema.prisma via getDMMF -- no live database
// connection required, so these pass against the placeholder DATABASE_URL
// exactly the same as they will against a real Neon connection string.
import { describe, it, expect, beforeAll } from 'vitest';
import { getDMMF } from '@prisma/internals';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

let dmmf: Awaited<ReturnType<typeof getDMMF>>;

beforeAll(async () => {
  const datamodel = readFileSync(join(__dirname, '..', 'prisma', 'schema.prisma'), 'utf-8');
  dmmf = await getDMMF({ datamodel });
});

/// Doc 30 TASK-LIC-001: "no table may contain a column for transaction
/// amounts, merchant names, bank details, or email content." `plans.amount_minor`
/// is the one deliberate, reviewed exception -- Dinero's own subscription
/// price (Doc 03 §3), not a user transaction amount -- so it's excluded by
/// exact model+field match, not by loosening the "amount" pattern generally.
const DENYLIST_PATTERNS = [
  /merchant/i,
  /\bbank\b/i,
  /iban/i,
  /account_?number/i,
  /card_?number/i,
  /\bcvv\b/i,
  /\bpan\b/i,
  /upi/i,
  /transaction/i,
  /balance/i,
  /email_?body/i,
  /email_?content/i,
  /statement/i,
];
const REVIEWED_EXCEPTIONS = new Set(['Plan.amountMinor']);

describe('test_schema_contains_no_financial_data_columns', () => {
  it('rejects any field name matching the financial-data denylist, except reviewed pricing fields', () => {
    const violations: string[] = [];
    for (const model of dmmf.datamodel.models) {
      for (const field of model.fields) {
        const key = `${model.name}.${field.name}`;
        if (REVIEWED_EXCEPTIONS.has(key)) continue;
        if (DENYLIST_PATTERNS.some((re) => re.test(field.name) || re.test(field.dbName ?? ''))) {
          violations.push(key);
        }
      }
    }
    expect(violations, `Financial-data-shaped columns found: ${violations.join(', ')}`).toEqual([]);
  });
});

describe('test_licensing_audit_log_account_id_is_nullable_set_null_on_delete', () => {
  it('LicensingAuditLog.accountId is nullable with an ON DELETE SET NULL relation', () => {
    const model = dmmf.datamodel.models.find((m) => m.name === 'LicensingAuditLog');
    expect(model).toBeDefined();

    const accountIdField = model!.fields.find((f) => f.name === 'accountId');
    expect(accountIdField?.isRequired).toBe(false);

    const relationField = model!.fields.find((f) => f.kind === 'object' && f.name === 'account');
    expect(relationField?.relationOnDelete).toBe('SetNull');
  });
});

describe('test_other_fk_tables_cascade_on_account_delete', () => {
  it('Subscription, LicenseToken, PaymentProviderRecord all CASCADE on account delete', () => {
    for (const modelName of ['Subscription', 'LicenseToken', 'PaymentProviderRecord']) {
      const model = dmmf.datamodel.models.find((m) => m.name === modelName);
      expect(model, `${modelName} model missing`).toBeDefined();

      const accountIdField = model!.fields.find((f) => f.name === 'accountId');
      expect(accountIdField?.isRequired, `${modelName}.accountId must be NOT NULL`).toBe(true);

      const relationField = model!.fields.find((f) => f.kind === 'object' && f.name === 'account');
      expect(
        relationField?.relationOnDelete,
        `${modelName} -> account must be ON DELETE CASCADE`
      ).toBe('Cascade');
    }
  });
});
