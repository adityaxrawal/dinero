import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import InstrumentCardHero from '@/components/instruments/InstrumentCardHero';
import type { InstrumentRecord } from '@/lib/ipc';

const toast = vi.fn();
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));

const instrument = (over: Partial<InstrumentRecord> = {}): InstrumentRecord => ({
  id: 'inst_1',
  instrument_type: 'credit_card',
  issuer_name: 'HDFC Bank',
  masked_identifier: '8841',
  status: 'active',
  current_balance: -1500,
  ...over,
});

// The hero's gradient is the only place issuer branding is expressed, so the
// theme lookup is asserted through the rendered class list.
const gradientOf = (issuer: string) => {
  const { container } = render(<InstrumentCardHero instrument={instrument({ issuer_name: issuer })} />);
  return container.firstElementChild!.className;
};

describe('InstrumentCardHero bank theming', () => {
  it.each([
    ['IDFC FIRST Bank', '#600C12'],
    ['HDFC Bank', '#0F3868'],
    ['SBI Cards', '#1E3A8A'],
    ['Axis Bank', '#6B0F38'],
    ['Jupiter', '#045C4B'],
    ['Yes Bank', '#1E3A5F'],
  ])('gives %s its own gradient', (issuer, startColor) => {
    expect(gradientOf(issuer)).toContain(startColor);
  });

  it('matches the issuer case-insensitively', () => {
    expect(gradientOf('hdfc bank')).toContain('#0F3868');
  });

  it('falls back to the Dinero emerald theme for an unknown issuer', () => {
    expect(gradientOf('Some Credit Union')).toContain('#064E3B');
  });
});

describe('InstrumentCardHero', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    toast.mockClear();
    Object.assign(navigator, { clipboard: { writeText: vi.fn() } });
  });
  afterEach(() => vi.useRealTimers());

  it('shows a masked identifier when no full number is stored', () => {
    render(<InstrumentCardHero instrument={instrument()} />);
    expect(screen.getByText('•••• •••• •••• 8841')).toBeTruthy();
  });

  it('prefers the full identifier when one is stored', () => {
    render(<InstrumentCardHero instrument={instrument({ full_identifier: '4532 7603 1920 8841' })} />);
    expect(screen.getByText('4532 7603 1920 8841')).toBeTruthy();
  });

  it('renders a negative balance as a debit with a minus sign', () => {
    render(<InstrumentCardHero instrument={instrument({ current_balance: -1500 })} />);
    expect(screen.getByText('−₹1,500.00')).toBeTruthy();
  });

  it('renders a positive balance without a sign', () => {
    render(<InstrumentCardHero instrument={instrument({ current_balance: 1500 })} />);
    expect(screen.getByText('₹1,500.00')).toBeTruthy();
  });

  it('labels a bank account balance as available rather than spent', () => {
    render(<InstrumentCardHero instrument={instrument({ instrument_type: 'bank_account' })} />);
    expect(screen.getByText('Available Balance')).toBeTruthy();
  });

  it('copies the identifier and confirms with a toast', () => {
    render(<InstrumentCardHero instrument={instrument({ full_identifier: '4532' })} />);
    fireEvent.click(screen.getByLabelText('Copy identifier'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('4532');
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Copied to clipboard' }));
  });

  it('does not copy when the instrument carries no identifier at all', () => {
    render(<InstrumentCardHero instrument={instrument({ masked_identifier: '' })} />);
    fireEvent.click(screen.getByLabelText('Copy identifier'));
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  describe('credit utilisation gauge', () => {
    it('is hidden without a credit limit', () => {
      render(<InstrumentCardHero instrument={instrument()} />);
      expect(screen.queryByText(/Limit:/)).toBeNull();
    });

    it('is hidden for non-card instruments even when a limit exists', () => {
      render(
        <InstrumentCardHero
          instrument={instrument({ instrument_type: 'bank_account', credit_limit: 100000 })}
        />
      );
      expect(screen.queryByText(/Limit:/)).toBeNull();
    });

    it('reports the used percentage', () => {
      render(<InstrumentCardHero instrument={instrument({ current_balance: -25000, credit_limit: 100000 })} />);
      expect(screen.getByText(/25\.0% used/)).toBeTruthy();
    });

    it('warns only once utilisation passes 80%', () => {
      render(<InstrumentCardHero instrument={instrument({ current_balance: -50000, credit_limit: 100000 })} />);
      expect(screen.queryByText(/High credit utilization/)).toBeNull();
    });

    it('warns at high utilisation', () => {
      render(<InstrumentCardHero instrument={instrument({ current_balance: -90000, credit_limit: 100000 })} />);
      expect(screen.getByText(/High credit utilization/)).toBeTruthy();
    });

    it('caps the reported ratio at 100% when over limit', () => {
      render(<InstrumentCardHero instrument={instrument({ current_balance: -150000, credit_limit: 100000 })} />);
      expect(screen.getByText(/100\.0% used/)).toBeTruthy();
    });
  });

  describe('billing cycle countdown', () => {
    it('is hidden for non-credit-card instruments', () => {
      render(
        <InstrumentCardHero
          instrument={instrument({ instrument_type: 'bank_account', billing_cycle_day: 15 })}
        />
      );
      expect(screen.queryByText(/Bill in/)).toBeNull();
    });

    it('counts down to a cycle day later this month', () => {
      vi.setSystemTime(new Date(2026, 0, 10));
      render(<InstrumentCardHero instrument={instrument({ billing_cycle_day: 15 })} />);
      expect(screen.getByText(/Bill in 5 days/)).toBeTruthy();
    });

    it('rolls over into next month once the cycle day has passed', () => {
      // 20 Jan, cycle day 15 -> 31-day month -> 26 days to next cycle.
      vi.setSystemTime(new Date(2026, 0, 20));
      render(<InstrumentCardHero instrument={instrument({ billing_cycle_day: 15 })} />);
      expect(screen.getByText(/Bill in 26 days/)).toBeTruthy();
    });

    it('says the bill generated today on the cycle day', () => {
      vi.setSystemTime(new Date(2026, 0, 15));
      render(<InstrumentCardHero instrument={instrument({ billing_cycle_day: 15 })} />);
      expect(screen.getByText(/Bill generated today/)).toBeTruthy();
    });

    it('uses the singular day form', () => {
      vi.setSystemTime(new Date(2026, 0, 14));
      render(<InstrumentCardHero instrument={instrument({ billing_cycle_day: 15 })} />);
      expect(screen.getByText(/Bill in 1 day(?!s)/)).toBeTruthy();
    });
  });
});
