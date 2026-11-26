// Skipped emails with reasons, and trusting a rejected bank sender (Q-8).
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import SkippedEmailsSettings from '@/components/settings/SkippedEmailsSettings';

const trustSender = vi.fn();
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast: vi.fn() }) }));
vi.mock('@/lib/ipc', () => ({
  API: {
    skippedEmails: {
      list: () =>
        Promise.resolve([
          { message_id: 'a', sender_domain: 'mail.hsbc.bank.in', bank_name: null, subject: 'Transaction Limits', reason: 'Unlisted subdomain of hsbc.bank.in', gate: 'sender', skipped_at: null },
          { message_id: 'b', sender_domain: 'mail.hsbc.bank.in', bank_name: null, subject: 'Log on', reason: 'Unlisted subdomain of hsbc.bank.in', gate: 'sender', skipped_at: null },
          { message_id: 'c', sender_domain: 'sbicard.com', bank_name: 'SBI Card', subject: 'OTP for Zomato', reason: 'gate2a_reject_Otp', gate: 'content', skipped_at: null },
        ]),
      trustSender: (...a: unknown[]) => trustSender(...a),
    },
  },
}));

const renderIt = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SkippedEmailsSettings />
    </QueryClientProvider>
  );

beforeEach(() => trustSender.mockResolvedValue(2));

describe('SkippedEmailsSettings', () => {
  it('explains each skip and offers trust once per rejected sender', async () => {
    renderIt();
    expect(await screen.findByText('Transaction Limits')).toBeInTheDocument();
    expect(screen.getAllByText(/Unlisted address for a known bank/)).toHaveLength(2);
    expect(screen.getByText(/One-time password/)).toBeInTheDocument();
    const buttons = screen.getAllByRole('button', { name: 'Trust this sender' });
    expect(buttons).toHaveLength(1);
    fireEvent.click(buttons[0]);
    await waitFor(() => expect(trustSender).toHaveBeenCalledWith('mail.hsbc.bank.in'));
  });
});
