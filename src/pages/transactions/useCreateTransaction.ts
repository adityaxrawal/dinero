import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { getErrorToast } from '@/lib/errorMapping';
import { useToast } from '@/hooks/use-toast';

export function useCreateTransaction() {
  const { toast } = useToast();
  const queryClient = useQueryClient();

  const [isOpen, setIsOpen] = useState(false);
  const [amount, setAmount] = useState('');
  const [direction, setDirection] = useState<'debit' | 'credit'>('debit');
  const [merchant, setMerchant] = useState('');
  const [date, setDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [instrumentId, setInstrumentId] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const submit = async () => {
    const amountValue = parseFloat(amount);
    if (isNaN(amountValue) || amountValue <= 0 || !merchant.trim() || !instrumentId) return;
    setIsCreating(true);
    try {
      await API.transactions.create({
        amountMinor: Math.round(amountValue * 100),
        currency: 'INR',
        direction,
        eventTime: `${date} 00:00:00`,
        merchantName: merchant.trim(),
        instrumentId,
      });
      toast({ title: 'Transaction Created', description: 'Your manual entry has been added.' });
      setIsOpen(false);
      setAmount('');
      setMerchant('');
      setDirection('debit');
      setInstrumentId('');
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
    } catch (e) {
      toast({ variant: 'destructive', ...getErrorToast(e) });
    } finally {
      setIsCreating(false);
    }
  };

  return {
    isOpen,
    setIsOpen,
    amount,
    setAmount,
    direction,
    setDirection,
    merchant,
    setMerchant,
    date,
    setDate,
    instrumentId,
    setInstrumentId,
    isCreating,
    submit,
  };
}
