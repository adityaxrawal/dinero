import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import CardNumberInput from './CardNumberInput';

const toast = vi.fn();
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));

const setup = (value: string, onChange = vi.fn()) => {
  const utils = render(<CardNumberInput value={value} onChange={onChange} />);
  return { ...utils, onChange, input: screen.getByRole('textbox') as HTMLInputElement };
};

describe('CardNumberInput network detection', () => {
  const badgeFor = (value: string) => {
    const { container } = render(<CardNumberInput value={value} onChange={vi.fn()} />);
    return container.querySelector('span')!.textContent;
  };

  it.each([
    ['4532760319208841', 'VISA'],
    ['5412345678901234', 'MC'],
    ['2221000000000009', 'MC'],
    ['2720999999999999', 'MC'],
    ['341234567890123', 'AMEX'],
    ['371234567890123', 'AMEX'],
    ['6012345678901234', 'RuPay'],
    ['6512345678901234', 'RuPay'],
    ['5081234567890123', 'RuPay'],
    ['3528123456789012', 'RuPay'],
  ])('detects %s as %s', (number, label) => {
    expect(badgeFor(number)).toBe(label);
  });

  it('detects the network from a formatted value with spaces', () => {
    expect(badgeFor('4532 7603 1920 8841')).toBe('VISA');
  });

  it('falls back to a generic badge for an empty or unrecognised prefix', () => {
    expect(badgeFor('')).toBe('CARD');
    expect(badgeFor('9999888877776666')).toBe('CARD');
  });

  it('does not misread a Mastercard range boundary as Mastercard', () => {
    // 2220 sits just below the 2221-2720 Mastercard range.
    expect(badgeFor('2220000000000000')).toBe('CARD');
  });
});

describe('CardNumberInput formatting', () => {
  it('groups digits into 4-character blocks', () => {
    expect(setup('4532760319208841').input.value).toBe('4532 7603 1920 8841');
  });

  it('formats a partial number without a trailing space', () => {
    expect(setup('45327').input.value).toBe('4532 7');
  });

  it('caps input at 19 digits', () => {
    expect(setup('12345678901234567890123').input.value).toBe('1234 5678 9012 3456 789');
  });

  it('reports only digits back to the caller, stripping punctuation', () => {
    const { input, onChange } = setup('');
    fireEvent.change(input, { target: { value: '4532-7603' } });
    expect(onChange).toHaveBeenCalledWith('45327603');
  });
});

describe('CardNumberInput masking', () => {
  beforeEach(() => {
    toast.mockClear();
    Object.assign(navigator, { clipboard: { writeText: vi.fn() } });
  });

  it('shows the number unmasked by default', () => {
    expect(setup('4532760319208841').input.value).toBe('4532 7603 1920 8841');
  });

  it('masks every block but the last when toggled', () => {
    const { input } = setup('4532760319208841');
    fireEvent.click(screen.getByLabelText('Mask card number'));
    expect(input.value).toBe('•••• •••• •••• 8841');
  });

  it('keeps a short single-block value visible when masked', () => {
    const { input } = setup('4532');
    fireEvent.click(screen.getByLabelText('Mask card number'));
    expect(input.value).toBe('4532');
  });

  it('masks all but the last four of a single long block', () => {
    const { input } = setup('45327');
    fireEvent.click(screen.getByLabelText('Mask card number'));
    // "4532 7" -> two blocks, so the first is masked.
    expect(input.value).toBe('•••• 7');
  });

  it('unmasks automatically as soon as the user types', () => {
    const { input, onChange } = setup('4532760319208841');
    fireEvent.click(screen.getByLabelText('Mask card number'));
    fireEvent.change(input, { target: { value: '45327603192088412' } });
    expect(onChange).toHaveBeenCalled();
    expect(screen.getByLabelText('Mask card number')).toBeTruthy();
  });

  it('copies the raw digits, not the formatted display value', () => {
    setup('4532760319208841');
    fireEvent.click(screen.getByLabelText(/copy/i));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('4532760319208841');
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Card Number Copied' }));
  });

  it('does nothing when copying an empty field', () => {
    setup('');
    fireEvent.click(screen.getByLabelText(/copy/i));
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });
});
