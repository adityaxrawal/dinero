import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TransactionAmountBalance } from '@/components/transactions/TransactionAmountBalance';

const tx = (over = {}) => ({
  currency: 'INR',
  balance_after_transaction: null,
  original_amount_minor: null,
  original_currency: null,
  exchange_rate: null,
  ...over,
});

const renderRow = (over = {}, isForeignCurrency = false) =>
  render(<TransactionAmountBalance tx={tx(over)} isForeignCurrency={isForeignCurrency} />);

describe('TransactionAmountBalance', () => {
  it('renders nothing when there is neither a balance nor a foreign amount', () => {
    const { container } = renderRow();
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the currency once there is something to report', () => {
    renderRow({ balance_after_transaction: 15000 });
    expect(screen.getByText('INR')).toBeTruthy();
  });

  it('defaults a missing currency to INR', () => {
    renderRow({ currency: null, balance_after_transaction: 15000 });
    expect(screen.getByText('INR')).toBeTruthy();
  });

  it('formats the running balance with thousands separators', () => {
    renderRow({ balance_after_transaction: 15000 });
    expect(screen.getByText(/15,000\.00/)).toBeTruthy();
  });

  it('renders a zero balance rather than hiding the row', () => {
    renderRow({ balance_after_transaction: 0 });
    expect(screen.getByText(/0\.00/)).toBeTruthy();
  });

  it('hides the balance row when the bank did not report one', () => {
    renderRow({ original_amount_minor: 2500, original_currency: 'USD' }, true);
    expect(screen.queryByText('Balance After')).toBeNull();
  });

  describe('foreign currency', () => {
    const foreign = { original_amount_minor: 2500, original_currency: 'USD', exchange_rate: 83.2512 };

    it('shows the original amount in its own currency', () => {
      renderRow(foreign, true);
      expect(screen.getByText('$25.00')).toBeTruthy();
    });

    it('shows the exchange rate at four decimal places', () => {
      renderRow(foreign, true);
      expect(screen.getByText('83.2512')).toBeTruthy();
    });

    it('omits the rate row when the rate is unknown', () => {
      renderRow({ ...foreign, exchange_rate: null }, true);
      expect(screen.queryByText('Exchange Rate')).toBeNull();
      expect(screen.getByText('$25.00')).toBeTruthy();
    });

    it('shows nothing foreign-specific for a domestic transaction', () => {
      renderRow({ balance_after_transaction: 15000, ...foreign }, false);
      expect(screen.queryByText('Original Amount')).toBeNull();
      expect(screen.queryByText('Exchange Rate')).toBeNull();
    });
  });
});
