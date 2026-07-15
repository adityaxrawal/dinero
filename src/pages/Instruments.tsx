import { useEffect, useState, useCallback } from 'react';
import { API, InstrumentRecord } from '../lib/ipc';
import { CreditCard, Landmark, Smartphone, Plus, AlertTriangle, ChevronRight, ChevronDown, Trash2, Edit } from 'lucide-react';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useToast } from '@/hooks/use-toast';

const INSTRUMENT_TYPES = [
  { value: 'credit_card', label: 'Credit Card' },
  { value: 'debit_card', label: 'Debit Card' },
  { value: 'bank_account', label: 'Bank Account' },
  { value: 'upi_vpa', label: 'UPI VPA' },
];

function instrumentIcon(type: string) {
  switch (type) {
    case 'credit_card':
    case 'debit_card':
      return <CreditCard size={20} aria-hidden="true" />;
    case 'bank_account':
      return <Landmark size={20} aria-hidden="true" />;
    case 'upi_vpa':
      return <Smartphone size={20} aria-hidden="true" />;
    default:
      return <CreditCard size={20} aria-hidden="true" />;
  }
}

interface InstrumentFormState {
  instrumentType: string;
  // A bank is not a first-class entity — it is the issuer_name grouping of the
  // unified instruments table, auto-discovered from instruments rather than
  // chosen from a separate banks list (Doc 15 §2 principle 8).
  issuerName: string;
  maskedIdentifier: string;
  fullIdentifier?: string;
  billingCycleDay?: string;
  bankIfsc?: string;
}

const EMPTY_FORM: InstrumentFormState = {
  instrumentType: '',
  issuerName: '',
  maskedIdentifier: '',
  fullIdentifier: '',
  billingCycleDay: '',
  bankIfsc: '',
};

export default function Instruments() {
  const { toast } = useToast();
  const [instruments, setInstruments] = useState<InstrumentRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Expanded issuer groups (grouped by issuer_name — instruments are the only
  // source of truth for which issuers exist; Doc 15 §2 principle 8).
  const [expandedIssuers, setExpandedIssuers] = useState<Record<string, boolean>>({});

  const [addModalOpen, setAddModalOpen] = useState(false);
  const [addForm, setAddForm] = useState<InstrumentFormState>(EMPTY_FORM);

  const [editModalOpen, setEditModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editForm, setEditForm] = useState<InstrumentFormState>(EMPTY_FORM);

  const [deleteTarget, setDeleteTarget] = useState<InstrumentRecord | null>(null);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const instsData = await API.instruments.list();
      setInstruments(instsData);
    } catch (e: any) {
      setError(e?.message || 'Failed to load instruments data.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleAddInstrument = async () => {
    try {
      await API.instruments.create(
        addForm.instrumentType,
        addForm.issuerName,
        addForm.maskedIdentifier,
        addForm.fullIdentifier || undefined,
        addForm.billingCycleDay ? parseInt(addForm.billingCycleDay) : undefined,
        addForm.bankIfsc || undefined
      );
      toast({ title: 'Instrument added' });
      setAddModalOpen(false);
      setAddForm(EMPTY_FORM);
      loadData();
    } catch (err: any) {
      toast({ title: 'Error', description: err?.message, variant: 'destructive' });
    }
  };

  const handleEditInstrument = async () => {
    if (!editingId) return;
    try {
      // Doc 30 TASK-API-002: issuer_name/masked_identifier are identity
      // fields, never editable post-creation -- not sent to the backend.
      // FLAGGED for Area 9 (TASK-FE-011): `renderInstrumentForm` below is
      // still shared as-is between add/edit and visually offers editable
      // issuer/masked-identifier inputs in edit mode even though the
      // backend now silently ignores them -- disabling/hiding those two
      // fields specifically in edit mode is frontend presentation work
      // out of this IPC-layer task's scope.
      await API.instruments.update(
        editingId,
        editForm.fullIdentifier || undefined,
        editForm.billingCycleDay ? parseInt(editForm.billingCycleDay) : undefined,
        editForm.bankIfsc || undefined
      );
      toast({ title: 'Instrument updated' });
      setEditModalOpen(false);
      setEditingId(null);
      setEditForm(EMPTY_FORM);
      loadData();
    } catch (err: any) {
      toast({ title: 'Error', description: err?.message, variant: 'destructive' });
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await API.instruments.delete(deleteTarget.id);
      toast({ title: 'Instrument deleted' });
      setDeleteTarget(null);
      loadData();
    } catch (err: any) {
      toast({ title: 'Error', description: err?.message, variant: 'destructive' });
    }
  };

  const toggleIssuer = (issuerName: string) => {
    setExpandedIssuers(prev => ({ ...prev, [issuerName]: !prev[issuerName] }));
  };

  const renderInstrumentForm = (form: InstrumentFormState, setForm: (val: InstrumentFormState) => void) => {
    return (
      <div className="grid gap-4 py-4">
        <div className="grid gap-2">
          <Label htmlFor="issuerName">Issuer Name</Label>
          <Input
            id="issuerName"
            value={form.issuerName}
            onChange={(e) => setForm({ ...form, issuerName: e.target.value })}
            placeholder="e.g. HDFC Bank"
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="instType">Type</Label>
          <Select
            value={form.instrumentType}
            onValueChange={(val) => setForm({ ...form, instrumentType: val })}
          >
            <SelectTrigger id="instType"><SelectValue placeholder="Select type" /></SelectTrigger>
            <SelectContent>
              {INSTRUMENT_TYPES.map((t) => (
                <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-2">
          <Label htmlFor="maskedId">Masked Identifier (e.g. XXXX1234 or user@upi)</Label>
          <Input
            id="maskedId"
            value={form.maskedIdentifier}
            onChange={(e) => setForm({ ...form, maskedIdentifier: e.target.value })}
            placeholder="XXXX1234"
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="fullId">Full Identifier (Account / Card No) <span className="text-muted-foreground text-xs">(Optional)</span></Label>
          <Input
            id="fullId"
            value={form.fullIdentifier}
            onChange={(e) => setForm({ ...form, fullIdentifier: e.target.value })}
            placeholder="1234567890123456"
          />
        </div>
        {form.instrumentType === 'credit_card' && (
          <div className="grid gap-2">
            <Label htmlFor="billingCycle">Billing Cycle Day <span className="text-muted-foreground text-xs">(Optional)</span></Label>
            <Input
              id="billingCycle"
              type="number"
              min="1"
              max="31"
              value={form.billingCycleDay}
              onChange={(e) => setForm({ ...form, billingCycleDay: e.target.value })}
              placeholder="15"
            />
          </div>
        )}
        {form.instrumentType === 'bank_account' && (
          <div className="grid gap-2">
            <Label htmlFor="ifsc">IFSC Code <span className="text-muted-foreground text-xs">(Optional)</span></Label>
            <Input
              id="ifsc"
              value={form.bankIfsc}
              onChange={(e) => setForm({ ...form, bankIfsc: e.target.value })}
              placeholder="HDFC0001234"
            />
          </div>
        )}
      </div>
    );
  };

  // Instruments are grouped by issuer_name — the auto-discovered partition that
  // stands in for "bank" — rather than a separate banks list (Doc 15 §2 principle 8).
  const issuerNames = Array.from(new Set(instruments.map(i => i.issuer_name))).sort();

  const getInstrumentsByIssuer = (issuerName: string) => {
    return instruments.filter(i => i.issuer_name === issuerName);
  };

  if (loading) return <div className="p-8">Loading instruments...</div>;
  if (error) return <div className="p-8 text-red-700">Error: {error}</div>;

  return (
    <div className="p-8 max-w-5xl mx-auto space-y-6">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">Instruments</h1>
          <p className="text-muted-foreground mt-1">
            Manage your connected bank accounts, credit cards, and UPI VPAs.
          </p>
        </div>
        <div className="flex gap-2">
          <Button onClick={() => setAddModalOpen(true)}>
            <Plus className="mr-2 h-4 w-4" />
            Add Instrument
          </Button>
        </div>
      </div>

      <div className="space-y-4">
        {issuerNames.map(issuerName => (
          <Card key={issuerName} className="overflow-hidden">
            <div
              className="p-4 bg-muted/30 cursor-pointer flex items-center justify-between hover:bg-muted/50 transition-colors"
              onClick={() => toggleIssuer(issuerName)}
            >
              <div className="flex items-center gap-3">
                {expandedIssuers[issuerName] ? <ChevronDown size={20} /> : <ChevronRight size={20} />}
                <h2 className="text-lg font-semibold">{issuerName}</h2>
                <Badge variant="secondary">{getInstrumentsByIssuer(issuerName).length} Instruments</Badge>
              </div>
            </div>

            {expandedIssuers[issuerName] && (
              <CardContent className="p-0">
                <div className="divide-y divide-border">
                  {getInstrumentsByIssuer(issuerName).map((inst) => (
                    <InstrumentRow
                      key={inst.id}
                      inst={inst}
                      onEdit={() => {
                        setEditingId(inst.id);
                        setEditForm({
                          instrumentType: inst.instrument_type,
                          issuerName: inst.issuer_name,
                          maskedIdentifier: inst.masked_identifier,
                          fullIdentifier: inst.full_identifier || "",
                          billingCycleDay: inst.billing_cycle_day?.toString() || "",
                          bankIfsc: inst.bank_ifsc || "",
                        });
                        setEditModalOpen(true);
                      }}
                      onDelete={() => setDeleteTarget(inst)}
                    />
                  ))}
                </div>
              </CardContent>
            )}
          </Card>
        ))}
      </div>

      {/* Add Instrument Modal */}
      <Dialog open={addModalOpen} onOpenChange={setAddModalOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add Instrument</DialogTitle>
            <DialogDescription>Add a new credit card, bank account, or UPI VPA.</DialogDescription>
          </DialogHeader>
          {renderInstrumentForm(addForm, setAddForm)}
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddModalOpen(false)}>Cancel</Button>
            <Button onClick={handleAddInstrument}>Add Instrument</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Instrument Modal */}
      <Dialog open={editModalOpen} onOpenChange={setEditModalOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Instrument</DialogTitle>
            <DialogDescription>Update details for this instrument.</DialogDescription>
          </DialogHeader>
          {renderInstrumentForm(editForm, setEditForm)}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditModalOpen(false)}>Cancel</Button>
            <Button onClick={handleEditInstrument}>Save Changes</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Modal */}
      <Dialog open={!!deleteTarget} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Instrument</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete {deleteTarget?.masked_identifier}? This action cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>Cancel</Button>
            <Button variant="destructive" onClick={handleDelete}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function InstrumentRow({ inst, onEdit, onDelete }: { inst: InstrumentRecord, onEdit: () => void, onDelete: () => void }) {
  const isNegative = (inst.current_balance ?? 0) < 0;

  return (
    <div className="p-4 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
      <div className="flex items-center gap-4">
        <div className="p-2 bg-muted rounded-full">
          {instrumentIcon(inst.instrument_type)}
        </div>
        <div>
          <div className="flex items-center gap-2">
            <span className="font-medium text-base">
              {inst.full_identifier || inst.masked_identifier}
            </span>
            {INSTRUMENT_TYPES.find(t => t.value === inst.instrument_type)?.label && (
              <Badge variant="outline" className="text-xs">
                {INSTRUMENT_TYPES.find(t => t.value === inst.instrument_type)?.label}
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-4 mt-1 text-sm text-muted-foreground">
            {inst.bank_ifsc && (
              <span>IFSC: <span className="text-foreground/80">{inst.bank_ifsc}</span></span>
            )}
            {inst.billing_cycle_day && (
              <span>Billing Day: <span className="text-foreground/80">{inst.billing_cycle_day}</span></span>
            )}
            {inst.credit_limit !== undefined && inst.credit_limit !== null && (
              <span>Limit: <span className="text-foreground/80">₹{inst.credit_limit}</span></span>
            )}
          </div>
        </div>
      </div>
      
      <div className="flex items-center gap-6 self-stretch sm:self-auto w-full sm:w-auto justify-between sm:justify-end">
        <div className="text-right">
          <p className="text-xs text-muted-foreground">Current Balance</p>
          <div className="flex items-center gap-2 justify-end">
            {isNegative && (
              <div title="Negative balance detected. Upload a statement or add a missing transaction to reconcile.">
                <AlertTriangle size={16} className="text-red-700" />
              </div>
            )}
            <p className={`font-semibold ${isNegative ? 'text-red-700' : ''}`}>
              {inst.current_balance !== undefined && inst.current_balance !== null 
                ? `₹${inst.current_balance.toFixed(2)}` 
                : '---'}
            </p>
          </div>
        </div>
        <div className="flex gap-1">
          <Button variant="ghost" size="icon" onClick={onEdit}>
            <Edit size={16} />
          </Button>
          <Button variant="ghost" size="icon" className="text-red-700 hover:text-red-700 hover:bg-destructive/10" onClick={onDelete}>
            <Trash2 size={16} />
          </Button>
        </div>
      </div>
    </div>
  );
}
