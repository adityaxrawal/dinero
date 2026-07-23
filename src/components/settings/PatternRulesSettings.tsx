import { useEffect, useMemo, useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Loader2,
  Pencil,
  Plus,
  Search,
  ShieldCheck,
  Trash2,
  Wand2,
} from 'lucide-react';
import { API, PatternRule, PatternRuleField, PatternRuleTestResult } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';
import { useToast } from '@/hooks/use-toast';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';

const FIELD_OPTIONS: { value: PatternRuleField; label: string }[] = [
  { value: 'amount', label: 'Amount' },
  { value: 'merchant', label: 'Merchant' },
  { value: 'currency', label: 'Currency' },
  { value: 'direction', label: 'Direction' },
  { value: 'event_time', label: 'Event Time' },
];

function fieldLabel(fieldName: string): string {
  return FIELD_OPTIONS.find((f) => f.value === fieldName)?.label ?? fieldName;
}

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    trusted: 'bg-[#064E3B] text-white border-transparent',
    active: 'text-[#064E3B] border-[#064E3B]/40 bg-[#064E3B]/5',
    pending: 'text-amber-700 border-amber-300 bg-amber-50',
    flagged: 'text-red-700 border-red-300 bg-red-50',
    inactive: 'text-[#064E3B]/50 border-[#064E3B]/15 bg-transparent',
  };
  return (
    <Badge variant="outline" className={cn('capitalize font-semibold', styles[status])}>
      {status}
    </Badge>
  );
}

interface RuleGroup {
  key: string;
  bank_name: string;
  field_name: string;
  rules: PatternRule[];
}

function groupRules(rules: PatternRule[]): RuleGroup[] {
  const groups = new Map<string, RuleGroup>();
  for (const rule of rules) {
    const key = `${rule.bank_name}::${rule.field_name}`;
    const existing = groups.get(key);
    if (existing) {
      existing.rules.push(rule);
    } else {
      groups.set(key, {
        key,
        bank_name: rule.bank_name,
        field_name: rule.field_name,
        rules: [rule],
      });
    }
  }
  const list = Array.from(groups.values());
  // Groups awaiting review surface first, then alphabetical by bank.
  list.sort((a, b) => {
    const aPending = a.rules.some((r) => r.status === 'pending') ? 0 : 1;
    const bPending = b.rules.some((r) => r.status === 'pending') ? 0 : 1;
    if (aPending !== bPending) return aPending - bPending;
    return a.bank_name.localeCompare(b.bank_name);
  });
  return list;
}

// Live regex-vs-sample preview shared by the edit and create dialogs. Debounced
// so it doesn't re-invoke the backend on every keystroke.
function useRegexTest(regex: string, sampleBody: string) {
  const [result, setResult] = useState<PatternRuleTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (!regex.trim() || !sampleBody.trim()) {
      setResult(null);
      return;
    }
    let cancelled = false;
    setTesting(true);
    const timer = setTimeout(() => {
      API.patternRules
        .test(regex, sampleBody)
        .then((r) => {
          if (!cancelled) setResult(r);
        })
        .catch(() => {
          if (!cancelled) setResult(null);
        })
        .finally(() => {
          if (!cancelled) setTesting(false);
        });
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [regex, sampleBody]);

  return { result, testing };
}

function RegexTestPreview({ result, testing }: { result: PatternRuleTestResult | null; testing: boolean }) {
  if (testing) {
    return (
      <p className="text-[12px] font-medium text-[#064E3B]/50 flex items-center gap-1.5">
        <Loader2 className="w-3 h-3 animate-spin" /> Testing…
      </p>
    );
  }
  if (!result) {
    return (
      <p className="text-[12px] font-medium text-[#064E3B]/50">
        Paste a sample email body below to test this pattern.
      </p>
    );
  }
  if (!result.compiles) {
    return <p className="text-[12px] font-semibold text-red-600">Invalid regex: {result.error}</p>;
  }
  if (!result.matched || result.captured_value === null) {
    return (
      <p className="text-[12px] font-semibold text-amber-700">
        Compiles, but doesn't match the sample text.
      </p>
    );
  }
  return (
    <p className="text-[12px] font-semibold text-[#064E3B]">
      Captured: <span className="font-mono bg-[#064E3B]/10 px-1.5 py-0.5 rounded">{result.captured_value}</span>
    </p>
  );
}

export default function PatternRulesSettings() {
  const { toast } = useToast();
  const [rules, setRules] = useState<PatternRule[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busyId, setBusyId] = useState<string | null>(null);

  const [editingRule, setEditingRule] = useState<PatternRule | null>(null);
  const [editRegex, setEditRegex] = useState('');
  const [editSample, setEditSample] = useState('');
  const [editSaving, setEditSaving] = useState(false);
  const editTest = useRegexTest(editRegex, editSample);

  const [creating, setCreating] = useState(false);
  const [newBank, setNewBank] = useState('');
  const [newField, setNewField] = useState<PatternRuleField>('amount');
  const [newRegex, setNewRegex] = useState('');
  const [newSample, setNewSample] = useState('');
  const [createSaving, setCreateSaving] = useState(false);
  const createTest = useRegexTest(newRegex, newSample);

  const [deleteTarget, setDeleteTarget] = useState<PatternRule | null>(null);
  const [deleting, setDeleting] = useState(false);

  const loadRules = async () => {
    setIsLoading(true);
    try {
      setRules(await API.patternRules.list());
    } catch {
      // Ignore initial load error — the empty state covers it.
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadRules();
  }, []);

  const groups = useMemo(() => {
    const all = groupRules(rules);
    const q = search.trim().toLowerCase();
    if (!q) return all;
    return all.filter(
      (g) =>
        g.bank_name.toLowerCase().includes(q) ||
        g.rules.some((r) => r.rule_payload_json.regex?.toLowerCase().includes(q))
    );
  }, [rules, search]);

  const toggleExpanded = (key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleSetStatus = async (rule: PatternRule, newStatus: PatternRule['status']) => {
    setBusyId(rule.id);
    try {
      await API.patternRules.setStatus(rule.id, newStatus);
      await loadRules();
    } catch (err) {
      toast({
        variant: 'destructive',
        title: 'Failed to update rule',
        description: getErrorMessage(err),
      });
    } finally {
      setBusyId(null);
    }
  };

  const openEdit = (rule: PatternRule) => {
    setEditingRule(rule);
    setEditRegex(rule.rule_payload_json.regex ?? '');
    setEditSample('');
  };

  const handleSaveEdit = async () => {
    if (!editingRule) return;
    setEditSaving(true);
    try {
      await API.patternRules.updatePayload(editingRule.id, editRegex);
      toast({ title: 'Pattern updated', description: 'Saved — re-earning trust from pending.' });
      setEditingRule(null);
      await loadRules();
    } catch (err) {
      toast({ variant: 'destructive', title: 'Save failed', description: getErrorMessage(err) });
    } finally {
      setEditSaving(false);
    }
  };

  const handleCreate = async () => {
    setCreateSaving(true);
    try {
      await API.patternRules.create(newBank.trim(), newField, newRegex, newSample);
      toast({ title: 'Pattern created', description: `${newBank} / ${fieldLabel(newField)} is now active.` });
      setCreating(false);
      setNewBank('');
      setNewRegex('');
      setNewSample('');
      setNewField('amount');
      await loadRules();
    } catch (err) {
      toast({ variant: 'destructive', title: 'Create failed', description: getErrorMessage(err) });
    } finally {
      setCreateSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await API.patternRules.delete(deleteTarget.id);
      toast({ title: 'Pattern deleted' });
      setDeleteTarget(null);
      await loadRules();
    } catch (err) {
      toast({ variant: 'destructive', title: 'Delete failed', description: getErrorMessage(err) });
    } finally {
      setDeleting(false);
    }
  };

  return (
    <section>
      <div className="mb-6 flex items-start justify-between gap-4 flex-wrap">
        <div>
          <h2 className="text-xl font-bold flex items-center gap-2">
            <Wand2 className="w-5 h-5" /> Pattern Rules
          </h2>
          <p className="text-sm mt-1 text-[#064E3B]/70">
            One rule per bank and category — new email template formats merge in as additional
            variants automatically. Approve a pending candidate to put it to use, or write your own.
          </p>
        </div>
        <Button
          onClick={() => setCreating(true)}
          className="h-9 text-[13px] font-semibold flex-shrink-0"
        >
          <Plus className="w-4 h-4 mr-1.5" /> Add Pattern
        </Button>
      </div>

      {rules.length > 0 && (
        <div className="relative mb-4">
          <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-[#064E3B]/40" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search by bank name or regex…"
            className="pl-9 h-9 text-[13px]"
          />
        </div>
      )}

      {isLoading ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">Loading…</p>
      ) : groups.length === 0 ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">
          {rules.length === 0 ? 'No pattern rules learned yet.' : 'No patterns match your search.'}
        </p>
      ) : (
        <div className="space-y-2 max-h-[600px] overflow-y-auto pr-2">
          {groups.map((group) => {
            const isOpen = expanded.has(group.key);
            const pendingCount = group.rules.filter((r) => r.status === 'pending').length;
            return (
              <div
                key={group.key}
                className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 overflow-hidden"
              >
                <button
                  className="w-full flex items-center justify-between gap-3 p-3 text-left hover:bg-[#064E3B]/5 transition-colors"
                  onClick={() => toggleExpanded(group.key)}
                >
                  <div className="flex items-center gap-2 min-w-0">
                    {isOpen ? (
                      <ChevronDown className="w-4 h-4 flex-shrink-0 text-[#064E3B]/60" />
                    ) : (
                      <ChevronRight className="w-4 h-4 flex-shrink-0 text-[#064E3B]/60" />
                    )}
                    <span className="text-[14px] font-bold text-[#064E3B] truncate">
                      {group.bank_name}
                    </span>
                    <Badge variant="outline" className="text-[#064E3B]/70 border-[#064E3B]/20 flex-shrink-0">
                      {fieldLabel(group.field_name)}
                    </Badge>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    {pendingCount > 0 && (
                      <Badge variant="outline" className="text-amber-700 border-amber-300 bg-amber-50">
                        {pendingCount} pending
                      </Badge>
                    )}
                    <span className="text-[12px] font-medium text-[#064E3B]/50">
                      {group.rules.length} variant{group.rules.length === 1 ? '' : 's'}
                    </span>
                  </div>
                </button>

                {isOpen && (
                  <div className="border-t border-[#064E3B]/10 divide-y divide-[#064E3B]/10">
                    {group.rules.map((rule) => (
                      <div
                        key={rule.id}
                        className="p-3 pl-9 flex items-center justify-between gap-3 flex-wrap"
                      >
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="text-[11px] font-mono text-[#064E3B]/40 truncate">
                              {rule.template_hash.substring(0, 10)}…
                            </span>
                            <StatusBadge status={rule.status} />
                          </div>
                          <p className="text-[12px] font-mono text-[#064E3B]/70 truncate">
                            {rule.rule_payload_json.regex}
                          </p>
                          <p className="text-[11px] font-medium text-[#064E3B]/50 mt-0.5">
                            {rule.success_count} success · {rule.failure_count} failure ·{' '}
                            {(rule.confidence * 100).toFixed(0)}% confidence
                          </p>
                        </div>
                        <div className="flex items-center gap-1.5 flex-shrink-0">
                          {rule.status === 'pending' && (
                            <Button
                              size="sm"
                              variant="outline"
                              className="h-7 text-[12px] font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                              disabled={busyId === rule.id}
                              onClick={() => handleSetStatus(rule, 'active')}
                            >
                              <ShieldCheck className="w-3.5 h-3.5 mr-1" /> Approve
                            </Button>
                          )}
                          {(rule.status === 'active' || rule.status === 'trusted') && (
                            <Button
                              size="sm"
                              variant="outline"
                              className="h-7 text-[12px] font-semibold border-red-200 text-red-600 hover:bg-red-50 hover:border-red-300"
                              disabled={busyId === rule.id}
                              onClick={() => handleSetStatus(rule, 'inactive')}
                            >
                              Disable
                            </Button>
                          )}
                          {(rule.status === 'inactive' || rule.status === 'flagged') && (
                            <Button
                              size="sm"
                              variant="outline"
                              className="h-7 text-[12px] font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                              disabled={busyId === rule.id}
                              onClick={() => handleSetStatus(rule, 'active')}
                            >
                              Enable
                            </Button>
                          )}
                          <Button
                            size="sm"
                            variant="outline"
                            className="h-7 w-7 p-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                            onClick={() => openEdit(rule)}
                            title="Edit pattern"
                          >
                            <Pencil className="w-3.5 h-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            className="h-7 w-7 p-0 border-red-200 text-red-600 hover:bg-red-50 hover:border-red-300"
                            onClick={() => setDeleteTarget(rule)}
                            title="Delete pattern"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* ── Edit dialog ─────────────────────────────────────────────────── */}
      <Dialog open={!!editingRule} onOpenChange={(open) => !open && setEditingRule(null)}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>
              Edit {editingRule ? fieldLabel(editingRule.field_name) : ''} pattern —{' '}
              {editingRule?.bank_name}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            {editingRule && (editingRule.status === 'active' || editingRule.status === 'trusted') && (
              <p className="text-[12px] font-medium text-amber-700 bg-amber-50 border border-amber-200 rounded-lg p-2">
                This pattern is live. Saving resets it to <strong>pending</strong> so it re-earns
                trust before being used again.
              </p>
            )}
            <div>
              <Label className="text-[12px] font-semibold text-[#064E3B]/70">Regex</Label>
              <Textarea
                value={editRegex}
                onChange={(e) => setEditRegex(e.target.value)}
                className="font-mono text-[13px] mt-1"
                rows={2}
              />
            </div>
            <div>
              <Label className="text-[12px] font-semibold text-[#064E3B]/70">
                Sample email body (to test against)
              </Label>
              <Textarea
                value={editSample}
                onChange={(e) => setEditSample(e.target.value)}
                className="text-[13px] mt-1"
                rows={4}
                placeholder="Paste the email body this pattern is meant to match…"
              />
            </div>
            <RegexTestPreview result={editTest.result} testing={editTest.testing} />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditingRule(null)}>
              Cancel
            </Button>
            <Button onClick={handleSaveEdit} disabled={editSaving || !editRegex.trim()}>
              {editSaving ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Save'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Create dialog ───────────────────────────────────────────────── */}
      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>Add Pattern</DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <Label className="text-[12px] font-semibold text-[#064E3B]/70">Bank name</Label>
                <Input
                  value={newBank}
                  onChange={(e) => setNewBank(e.target.value)}
                  placeholder="e.g. HDFC Bank"
                  className="mt-1"
                />
              </div>
              <div>
                <Label className="text-[12px] font-semibold text-[#064E3B]/70">Field</Label>
                <Select value={newField} onValueChange={(v) => setNewField(v as PatternRuleField)}>
                  <SelectTrigger className="mt-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {FIELD_OPTIONS.map((f) => (
                      <SelectItem key={f.value} value={f.value}>
                        {f.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div>
              <Label className="text-[12px] font-semibold text-[#064E3B]/70">
                Regex (group 1 must capture the value)
              </Label>
              <Textarea
                value={newRegex}
                onChange={(e) => setNewRegex(e.target.value)}
                className="font-mono text-[13px] mt-1"
                rows={2}
                placeholder={String.raw`e.g. Rs\.?\s*([\d,]+\.\d{2})`}
              />
            </div>
            <div>
              <Label className="text-[12px] font-semibold text-[#064E3B]/70">
                Sample email body
              </Label>
              <Textarea
                value={newSample}
                onChange={(e) => setNewSample(e.target.value)}
                className="text-[13px] mt-1"
                rows={4}
                placeholder="Paste a real email body — this both tests the regex and identifies which template it applies to."
              />
            </div>
            <RegexTestPreview result={createTest.result} testing={createTest.testing} />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreating(false)}>
              Cancel
            </Button>
            <Button
              onClick={handleCreate}
              disabled={
                createSaving ||
                !newBank.trim() ||
                !newRegex.trim() ||
                !newSample.trim() ||
                !createTest.result?.matched
              }
            >
              {createSaving ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Delete confirmation ─────────────────────────────────────────── */}
      <Dialog open={!!deleteTarget} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Delete pattern?</DialogTitle>
          </DialogHeader>
          <p className="text-[13px] text-[#064E3B]/70">
            Permanently deletes this <strong>{deleteTarget && fieldLabel(deleteTarget.field_name)}</strong>{' '}
            pattern variant for <strong>{deleteTarget?.bank_name}</strong>. This can't be undone.
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              Cancel
            </Button>
            <Button
              onClick={handleDelete}
              disabled={deleting}
              className="bg-red-600 hover:bg-red-700 text-white"
            >
              {deleting ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Delete'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
