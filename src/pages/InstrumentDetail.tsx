/**
 * Full-page view of one payment instrument.
 */
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useInstrumentForm } from '@/components/instruments/useInstrumentForm';
import { instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import EditableDetailsCard from './instrumentDetail/EditableDetailsCard';
import SavedPasswordsCard from './instrumentDetail/SavedPasswordsCard';
import RecentTransactionsCard from './instrumentDetail/RecentTransactionsCard';
import StatementHistoryCard from './instrumentDetail/StatementHistoryCard';

/** Full-page view of one payment instrument. */
export default function InstrumentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { toast } = useToast();
  const form = useInstrumentForm(id, undefined, () => navigate('/instruments'));
  const { inst } = form;

  if (!id) return null;
  if (form.isLoading || !inst) {
    return (
      <div
        className="flex items-center justify-center h-40"
        role="status"
        aria-label="Loading instrument"
      >
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  /** Forgets a saved statement password for this instrument. */
  const handleForgetPassword = (passwordId: string) => {
    form.forgetPassword.mutate(passwordId, {
      onSuccess: () => toast({ title: 'Saved password forgotten' }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  return (
    <div className="flex-1 h-full overflow-y-auto">
      <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl mx-auto p-6 lg:p-10">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => navigate('/instruments')}
          aria-label="Back to instruments"
        >
          <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
        </Button>

        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-2xl font-bold">{inst.issuer_name}</h1>
            <Badge variant="outline">{instrumentTypeLabel(inst.instrument_type)}</Badge>
          </div>
          <p className="text-muted-foreground">{inst.masked_identifier}</p>
        </div>

        <EditableDetailsCard form={form} inst={inst} />

        {form.instrumentPasswords.length > 0 && (
          <SavedPasswordsCard
            passwords={form.instrumentPasswords}
            onForget={handleForgetPassword}
            isForgetting={form.forgetPassword.isPending}
          />
        )}

        <RecentTransactionsCard
          form={form}
          onViewAll={() => navigate(`/transactions?instrument=${id}`)}
        />

        <StatementHistoryCard statements={form.instrumentStatements} />
      </div>
    </div>
  );
}
