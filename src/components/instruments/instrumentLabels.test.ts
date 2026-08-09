// A UPI VPA carries no issuer field, so the handle is the only signal for what
// to call it — that mapping is what these pin.
import { describe, it, expect } from 'vitest';
import { getInstrumentTitle, getInstrumentSubtitle } from './instrumentLabels';
import { buildInstrumentUpdate, type InstrumentFormFields } from './instrumentUpdate';

const upi = (masked: string) => ({ instrument_type: 'upi_vpa', masked_identifier: masked });

describe('getInstrumentTitle', () => {
  it('prompts when there is no instrument at all', () => {
    expect(getInstrumentTitle()).toBe('Select Instrument');
  });

  it('prefers a real issuer name', () => {
    expect(getInstrumentTitle({ issuer_name: 'HDFC', instrument_type: 'credit_card' })).toBe('HDFC');
  });

  it('ignores a whitespace-only issuer name', () => {
    expect(getInstrumentTitle({ issuer_name: '   ', instrument_type: 'credit_card' })).toBe(
      'Credit Card'
    );
  });

  it('names the bank behind each known UPI handle', () => {
    expect(getInstrumentTitle(upi('me@jupiter'))).toBe('Jupiter UPI');
    expect(getInstrumentTitle(upi('me@okicici'))).toBe('ICICI UPI');
    expect(getInstrumentTitle(upi('me@icici'))).toBe('ICICI UPI');
    expect(getInstrumentTitle(upi('me@okaxis'))).toBe('Axis UPI');
    expect(getInstrumentTitle(upi('me@oksbi'))).toBe('SBI UPI');
    expect(getInstrumentTitle(upi('me@paytm'))).toBe('Paytm UPI');
    expect(getInstrumentTitle(upi('me@hdfc'))).toBe('HDFC UPI');
  });

  it('matches a handle regardless of case', () => {
    expect(getInstrumentTitle(upi('ME@OKAXIS'))).toBe('Axis UPI');
  });

  it('falls back to a generic label for an unrecognised handle', () => {
    expect(getInstrumentTitle(upi('me@somethingnew'))).toBe('UPI Payment Handle');
  });

  it('falls back to the type label when a VPA has no handle stored', () => {
    expect(getInstrumentTitle({ instrument_type: 'upi_vpa', masked_identifier: null })).toBe(
      'UPI VPA'
    );
  });
});

describe('getInstrumentSubtitle', () => {
  it('invites assignment when nothing is selected', () => {
    expect(getInstrumentSubtitle()).toBe('Click to assign');
  });

  it('appends the identifier when there is one', () => {
    expect(
      getInstrumentSubtitle({ instrument_type: 'credit_card', masked_identifier: '1234' })
    ).toBe('Credit Card · 1234');
  });

  it('shows the bare type when there is not', () => {
    expect(getInstrumentSubtitle({ instrument_type: 'credit_card', masked_identifier: null })).toBe(
      'Credit Card'
    );
  });
});

describe('buildInstrumentUpdate', () => {
  const empty: InstrumentFormFields = {
    issuerName: '',
    maskedIdentifier: '',
    nickname: '',
    fullIdentifier: '',
    billingCycleDay: '',
    bankIfsc: '',
    instrumentType: '',
    status: '',
    creditLimit: '',
    network: '',
    accountType: '',
    upiVpa: '',
    rewardsSummary: '',
    statementDueDate: '',
    minimumDue: '',
  };

  it('omits every blank field rather than sending empty strings', () => {
    expect(buildInstrumentUpdate(empty)).toEqual({});
  });

  it('maps camelCase fields onto their snake_case wire names', () => {
    expect(
      buildInstrumentUpdate({ ...empty, nickname: 'Travel', statementDueDate: '2026-08-01' })
    ).toEqual({ nickname: 'Travel', statement_due_date: '2026-08-01' });
  });

  it('parses the two money fields into numbers', () => {
    expect(buildInstrumentUpdate({ ...empty, creditLimit: '150000', minimumDue: '1200.50' })).toEqual(
      { credit_limit: 150000, minimum_due: 1200.5 }
    );
  });
});
