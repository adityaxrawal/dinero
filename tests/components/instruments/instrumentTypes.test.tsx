import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { INSTRUMENT_TYPES, instrumentTypeLabel, instrumentIcon } from '@/components/instruments/instrumentTypes';

describe('instrumentTypeLabel', () => {
  it.each(INSTRUMENT_TYPES)('labels $value as $label', ({ value, label }) => {
    expect(instrumentTypeLabel(value)).toBe(label);
  });

  it('echoes an unknown type rather than rendering an empty label', () => {
    expect(instrumentTypeLabel('crypto_wallet')).toBe('crypto_wallet');
  });
});

describe('instrumentIcon', () => {
  const iconOf = (type: string, size?: number) => {
    const { container } = render(<span>{instrumentIcon(type, size)}</span>);
    return container.querySelector('svg')!;
  };

  it.each(['credit_card', 'debit_card', 'bank_account', 'upi_vpa', 'crypto_wallet'])(
    'renders an icon for %s',
    (type) => {
      expect(iconOf(type)).toBeTruthy();
    }
  );

  it('gives card and account types visually distinct icons', () => {
    const card = iconOf('credit_card').innerHTML;
    const bank = iconOf('bank_account').innerHTML;
    const upi = iconOf('upi_vpa').innerHTML;
    expect(new Set([card, bank, upi]).size).toBe(3);
  });

  it('shares one icon between credit and debit cards', () => {
    expect(iconOf('credit_card').innerHTML).toBe(iconOf('debit_card').innerHTML);
  });

  it('falls back to the card icon for unknown types', () => {
    expect(iconOf('crypto_wallet').innerHTML).toBe(iconOf('credit_card').innerHTML);
  });

  it('defaults to size 20 and honours an override', () => {
    expect(iconOf('credit_card').getAttribute('width')).toBe('20');
    expect(iconOf('credit_card', 32).getAttribute('width')).toBe('32');
  });

  it('hides the icon from assistive tech since the label carries the meaning', () => {
    expect(iconOf('credit_card').getAttribute('aria-hidden')).toBe('true');
  });
});
