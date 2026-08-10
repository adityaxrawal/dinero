/**
 * Editable instrument fields on the detail page.
 */
import { Save, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import type { InstrumentRecord } from '@/lib/ipc';
import type { useInstrumentForm } from '@/components/instruments/useInstrumentForm';

type Form = ReturnType<typeof useInstrumentForm>;

/** Formats a monetary field, or a dash when absent. */
function money(value: number | null | undefined): string {
  return value != null ? `₹${value.toFixed(2)}` : '—';
}

/** Editable instrument fields. */
export default function EditableDetailsCard({
  form,
  inst,
}: {
  form: Form;
  inst: InstrumentRecord;
}) {
  const { fields, setField } = form;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Details</CardTitle>
        <CardDescription>
          Issuer and identifier are set at first-sight discovery and can't be edited.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <span className="text-muted-foreground">Current Balance</span>
            <p className="font-semibold">{money(inst.current_balance)}</p>
          </div>
          {inst.credit_limit != null && (
            <div>
              <span className="text-muted-foreground">Credit Limit</span>
              <p className="font-semibold">{money(inst.credit_limit)}</p>
            </div>
          )}
        </div>

        <div className="space-y-2">
          <Label htmlFor="fullId">Full Identifier</Label>
          <Input
            id="fullId"
            value={fields.fullIdentifier}
            onChange={(e) => setField('fullIdentifier', e.target.value)}
          />
        </div>

        {inst.instrument_type === 'credit_card' && (
          <div className="space-y-2">
            <Label htmlFor="billingCycle">Billing Cycle Day</Label>
            <Input
              id="billingCycle"
              type="number"
              min="1"
              max="31"
              value={fields.billingCycleDay}
              onChange={(e) => setField('billingCycleDay', e.target.value)}
            />
          </div>
        )}

        {inst.instrument_type === 'bank_account' && (
          <div className="space-y-2">
            <Label htmlFor="ifsc">IFSC Code</Label>
            <Input
              id="ifsc"
              value={fields.bankIfsc}
              onChange={(e) => setField('bankIfsc', e.target.value)}
            />
          </div>
        )}

        <Button onClick={form.handleSave} disabled={form.isSaving}>
          {form.isSaving ? (
            'Saving...'
          ) : (
            <>
              <Save className="w-4 h-4 mr-2" /> Save Changes
            </>
          )}
        </Button>
        <Button
          variant="outline"
          className="w-full text-red-700 hover:text-red-700"
          onClick={form.handleDelete}
          disabled={form.isDeleting}
        >
          <Trash2 className="w-4 h-4 mr-2" />{' '}
          {form.isDeleting ? 'Deleting...' : 'Delete Instrument'}
        </Button>
      </CardContent>
    </Card>
  );
}
