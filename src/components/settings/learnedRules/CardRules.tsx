/**
 * "Always use this card" rules the user confirmed for alerts that name the
 * bank and card type but no card number (owner decision Q-6). Revertable.
 */
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { CreditCard } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';

const CARD_RULES_KEY = ['instrumentRules'] as const;

export default function CardRules() {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const { data: rules = [] } = useQuery({ queryKey: CARD_RULES_KEY, queryFn: API.instrumentRules.list });
  if (rules.length === 0) return null;

  const revert = (id: string) =>
    API.instrumentRules
      .revert(id)
      .then(() => {
        queryClient.invalidateQueries({ queryKey: CARD_RULES_KEY });
        toast({ title: 'Rule removed', description: 'Already-saved transactions are unchanged.' });
      })
      .catch((err) => toast({ variant: 'destructive', ...getErrorToast(err) }));

  return (
    <div className="mt-8" aria-label="Card rules">
      <p className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-[#064E3B]/70">
        <CreditCard className="h-3.5 w-3.5" aria-hidden="true" /> Card rules
      </p>
      <ul className="divide-y divide-[#064E3B]/10 rounded-xl border border-[#064E3B]/10">
        {rules.map((r) => (
          <li key={r.id} className="flex items-center gap-2 p-3 text-[13px] text-[#064E3B]">
            <span className="flex-1">
              {r.issuer_name} {r.instrument_type.replace('_', ' ')} alerts without a card number →{' '}
              <strong className="font-semibold">{r.instrument_label}</strong>
            </span>
            <Button size="sm" variant="ghost" onClick={() => revert(r.id)}>
              Remove
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
