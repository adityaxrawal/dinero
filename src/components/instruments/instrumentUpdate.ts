/**
 * Builds the update payload for an edited instrument.
 *
 * Sends only changed fields, so an edit to one value cannot inadvertently
 * overwrite another with a stale one.
 */
export interface InstrumentFormFields {
  issuerName: string;
  maskedIdentifier: string;
  nickname: string;
  fullIdentifier: string;
  billingCycleDay: string;
  bankIfsc: string;
  instrumentType: string;
  status: string;
  creditLimit: string;
  network: string;
  accountType: string;
  upiVpa: string;
  rewardsSummary: string;
  statementDueDate: string;
  minimumDue: string;
}

export interface InstrumentUpdateExtra {
  nickname?: string;
  credit_limit?: number;
  account_type?: string;
  network?: string;
  status?: string;
  upi_vpa?: string;
  rewards_summary?: string;
  instrument_type?: string;
  issuer_name?: string;
  masked_identifier?: string;
  statement_due_date?: string;
  minimum_due?: number;
}

const TEXT_FIELDS: [keyof InstrumentFormFields, keyof InstrumentUpdateExtra][] = [
  ['nickname', 'nickname'],
  ['accountType', 'account_type'],
  ['network', 'network'],
  ['status', 'status'],
  ['upiVpa', 'upi_vpa'],
  ['rewardsSummary', 'rewards_summary'],
  ['instrumentType', 'instrument_type'],
  ['issuerName', 'issuer_name'],
  ['maskedIdentifier', 'masked_identifier'],
  ['statementDueDate', 'statement_due_date'],
];

const NUMERIC_FIELDS: [keyof InstrumentFormFields, keyof InstrumentUpdateExtra][] = [
  ['creditLimit', 'credit_limit'],
  ['minimumDue', 'minimum_due'],
];

/**
 * Builds the update payload from edited fields.
 *
 * Sends only what changed, so editing one field cannot overwrite another with a
 * stale value.
 */
export function buildInstrumentUpdate(fields: InstrumentFormFields): InstrumentUpdateExtra {
  const extra = {} as Record<string, string | number>;

  for (const [from, to] of TEXT_FIELDS) {
    if (fields[from]) extra[to] = fields[from];
  }
  for (const [from, to] of NUMERIC_FIELDS) {
    if (fields[from]) extra[to] = parseFloat(fields[from]);
  }

  return extra as InstrumentUpdateExtra;
}
