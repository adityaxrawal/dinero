import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, Loader2, Save, Trash2, KeyRound } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { queryKeys } from '@/lib/queryKeys';
import { useInstrumentForm } from '@/components/instruments/useInstrumentForm';
import { instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { cn } from '@/lib/utils';

/**
 * TASK-FE-011 (Doc 30): instrument-scoped transaction history (reusing the
 * TASK-FE-009 infinite-list infra filtered by instrument_id), statement
 * history, and a "forget saved password" action. Editable fields limited to
 * full_identifier/billing_cycle_day/bank_ifsc — issuer_name/masked_identifier
 * are identity fields the backend has never accepted post-creation
 * (Document 15 §2.8's resolve_instrument() matching key); the pre-existing
 * page's edit form offered them anyway despite the backend silently
 * discarding them, a gap explicitly flagged back in TASK-API-002 as this
 * task's to fix.
 */
export default function InstrumentDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { toast } = useToast();
  const {
    inst,
    isLoading,
    forgetPassword,
    fullIdentifier,
    setFullIdentifier,
    billingCycleDay,
    setBillingCycleDay,
    bankIfsc,
    setBankIfsc,
    isSaving,
    isDeleting,
    recentTransactions,
    instrumentStatements,
    instrumentPasswords,
    handleSave,
    handleDelete,
  } = useInstrumentForm(id, undefined, () => navigate('/instruments'));

  if (!id) return null;
  if (isLoading || !inst) {
    return (
      <div className="flex items-center justify-center h-40" role="status" aria-label="Loading instrument">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  const handleForgetPassword = (passwordId: string) => {
    forgetPassword.mutate(passwordId, {
      onSuccess: () => toast({ title: 'Saved password forgotten' }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  return (
    // AppLayout's <main> is overflow-hidden -- every routed page owns its
    // own scroll container, or content past the viewport is unreachable.
    <div className="flex-1 h-full overflow-y-auto">
    <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl mx-auto p-6 lg:p-10">
      <Button variant="ghost" size="sm" onClick={() => navigate('/instruments')} aria-label="Back to instruments">
        <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
      </Button>

      <div>
        <div className="flex items-center gap-2">
          <h1 className="text-2xl font-bold">{inst.issuer_name}</h1>
          <Badge variant="outline">{instrumentTypeLabel(inst.instrument_type)}</Badge>
        </div>
        <p className="text-muted-foreground">{inst.masked_identifier}</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Details</CardTitle>
          <CardDescription>Issuer and identifier are set at first-sight discovery and can't be edited.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">Current Balance</span>
              <p className="font-semibold">{inst.current_balance != null ? `₹${inst.current_balance.toFixed(2)}` : '—'}</p>
            </div>
            {inst.credit_limit != null && (
              <div>
                <span className="text-muted-foreground">Credit Limit</span>
                <p className="font-semibold">₹{inst.credit_limit.toFixed(2)}</p>
              </div>
            )}
          </div>

          <div className="space-y-2">
            <Label htmlFor="fullId">Full Identifier</Label>
            <Input id="fullId" value={fullIdentifier} onChange={(e) => setFullIdentifier(e.target.value)} />
          </div>
          {inst.instrument_type === 'credit_card' && (
            <div className="space-y-2">
              <Label htmlFor="billingCycle">Billing Cycle Day</Label>
              <Input id="billingCycle" type="number" min="1" max="31" value={billingCycleDay} onChange={(e) => setBillingCycleDay(e.target.value)} />
            </div>
          )}
          {inst.instrument_type === 'bank_account' && (
            <div className="space-y-2">
              <Label htmlFor="ifsc">IFSC Code</Label>
              <Input id="ifsc" value={bankIfsc} onChange={(e) => setBankIfsc(e.target.value)} />
            </div>
          )}

          <Button onClick={handleSave} disabled={isSaving}>
            {isSaving ? 'Saving...' : <><Save className="w-4 h-4 mr-2" /> Save Changes</>}
          </Button>
          <Button variant="outline" className="w-full text-red-700 hover:text-red-700" onClick={handleDelete} disabled={isDeleting}>
            <Trash2 className="w-4 h-4 mr-2" /> {isDeleting ? 'Deleting...' : 'Delete Instrument'}
          </Button>
        </CardContent>
      </Card>

      {instrumentPasswords.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Saved Statement Password</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {instrumentPasswords.map((p) => (
              <div key={p.id} className="flex items-center justify-between p-2 rounded-md border border-border">
                <div className="flex items-center gap-2">
                  <KeyRound className="w-4 h-4 text-muted-foreground" aria-hidden="true" />
                  <span className="text-sm">Used {p.success_count} time{p.success_count === 1 ? '' : 's'}</span>
                </div>
                <Button variant="outline" size="sm" onClick={() => handleForgetPassword(p.id)} disabled={forgetPassword.isPending}>
                  Forget Password
                </Button>
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle>Recent Transactions</CardTitle>
          <Button variant="link" size="sm" onClick={() => navigate(`/transactions?instrument=${id}`)}>
            View All
          </Button>
        </CardHeader>
        <CardContent>
          {recentTransactions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No transactions for this instrument yet.</p>
          ) : (
            <ul className="space-y-2">
              {recentTransactions.map((tx) => (
                <li key={tx.id} className="flex items-center justify-between text-sm">
                  <span>{tx.merchant}</span>
                  <span className={cn('font-medium', tx.direction === 'debit' ? 'text-red-700' : 'text-emerald-700')}>
                    {tx.direction === 'debit' ? '- ' : '+ '}₹{Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Statement History</CardTitle>
        </CardHeader>
        <CardContent>
          {instrumentStatements.length === 0 ? (
            <p className="text-sm text-muted-foreground">No statements for this instrument yet.</p>
          ) : (
            <ul className="space-y-2">
              {instrumentStatements.map((s) => (
                <li key={s.id} className="flex items-center justify-between text-sm">
                  <span>{s.file_name}</span>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">{formatCustomDate(s.date)}</span>
                    <Badge variant={s.status === 'PROCESSED' ? 'default' : 'secondary'} className="text-[10px]">
                      {s.status}
                    </Badge>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
    </div>
  );
}
