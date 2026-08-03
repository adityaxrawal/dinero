import { useState, useEffect } from 'react';
import { X, AlertTriangle, FileText, Ban, Save, CheckCircle2, XCircle } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useResolveUnassignedTransaction } from '@/hooks/mutations/useResolveUnassignedTransaction';
import { DatePicker } from '@/components/ui/date-picker';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { InstrumentPicker } from '@/components/instruments/InstrumentPicker';
import { GmailEmailViewer } from '@/components/common/GmailEmailViewer';

interface UnassignedInspectorProps {
  record: UnassignedTransactionRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

export default function UnassignedInspector({
  record,
  onClose,
  inline = false,
}: UnassignedInspectorProps) {
  const queryClient = useQueryClient();
  const isOpen = !!record;
  const { toast } = useToast();
  
  const { data: instruments = [] } = useInstrumentsList();
  const resolveManually = useResolveUnassignedTransaction();

  const [merchant, setMerchant] = useState('');
  const [amount, setAmount] = useState('');
  const [direction, setDirection] = useState<'debit' | 'credit'>('debit');
  const [date, setDate] = useState('');
  const [instrumentId, setInstrumentId] = useState('');
  const [referenceId, setReferenceId] = useState('');

  useEffect(() => {
    setMerchant(record?.merchant_raw ?? '');
    setAmount(record?.amount_minor != null ? (record.amount_minor / 100).toString() : '');
    setDirection(record?.direction === 'credit' ? 'credit' : 'debit');
    setDate(record?.event_time ? record.event_time.slice(0, 10) : '');
    setInstrumentId('');
    setReferenceId('');
  }, [record?.id, record?.merchant_raw, record?.amount_minor, record?.direction, record?.event_time]);

  const canSubmit = merchant.trim() && amount && date && instrumentId;

  const asideClasses = cn(
    !inline && 'inspector-panel',
    !inline && (!record || !isOpen) && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );

  const asideStyle = inline
    ? { backgroundColor: '#F8E7C9' }
    : { width: record && isOpen ? 'var(--inspector-width)' : 0 };

  const handleDismiss = async () => {
    if (!record) return;
    try {
      await API.reconciliation.dismissUnassigned(record.id);
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unassigned() });
      onClose();
    } catch (err) {
      console.error('Failed to dismiss', err);
    }
  };

  const handleSave = () => {
    if (!record || !canSubmit) return;
    resolveManually.mutate(
      {
        id: record.id,
        amountMinor: Math.round(parseFloat(amount) * 100),
        currency: record.currency ?? 'INR',
        direction,
        eventTime: `${date} 00:00:00`,
        merchantName: merchant.trim(),
        instrumentId,
        referenceId: referenceId.trim() || undefined,
      },
      {
        onSuccess: () => {
          toast({ title: 'Transaction saved' });
          onClose();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  if (!record) {
    return (
      <aside className={asideClasses} role="complementary" aria-hidden={true} style={asideStyle} />
    );
  }

  const reasonTitleMap: Record<string, string> = {
    extraction_failed: 'Failed to Extract Details',
    issuer_name_not_found: 'Unknown Payment Instrument',
    pending_llm_enrichment: 'Pending Background AI Enrichment',
    'gate3_failed:missing_amount': 'Missing Transaction Amount',
    'gate3_failed:missing_counterparty': 'Missing Counterparty / Merchant',
    'gate3_failed:missing_instrument': 'Missing Instrument Details',
    gate3_failed: 'Mandatory Field Gate Failed',
  };

  const title = reasonTitleMap[record.reason] || record.reason || 'Unresolved Issue';

  let emailHtml = '';
  let emailText = '';
  let emailSubject = '';
  let emailSender = '';
  if (record.raw_payload_json) {
    try {
      const parsed = JSON.parse(record.raw_payload_json);
      emailHtml = parsed.html || '';
      emailText = parsed.body || parsed.snippet || '';
      emailSubject = parsed.subject || '';
      emailSender = parsed.sender || parsed.from || '';
    } catch (e) {
      emailText = record.body_snippet || '';
    }
  } else if (record.body_snippet) {
    emailText = record.body_snippet;
  }

  // Diagnostic Checklist evaluations
  const checks = [
    {
      id: 'gate1',
      label: 'Sender Verification (Gate 1)',
      passed: true,
      value: 'Verified Bank Alert Domain',
    },
    {
      id: 'gate2',
      label: 'Content Classifier (Gate 2)',
      passed: true,
      value: 'Transaction Alert / Update',
    },
    {
      id: 'amount',
      label: 'Transaction Amount',
      passed: record.amount_minor != null,
      value: record.amount_minor != null ? `${record.currency ?? 'INR'} ${(record.amount_minor / 100).toFixed(2)}` : 'Missing / Unparsed',
    },
    {
      id: 'merchant',
      label: 'Merchant / Counterparty',
      passed: !!record.merchant_raw,
      value: record.merchant_raw || 'Missing / Non-plausible',
    },
    {
      id: 'date',
      label: 'Transaction Date',
      passed: !!record.event_time,
      value: record.event_time ? record.event_time.slice(0, 10) : 'Missing / Unparsed',
    },
    {
      id: 'instrument',
      label: 'Instrument Resolution (Gate 4)',
      passed: record.reason !== 'issuer_name_not_found' && !record.reason.includes('missing_instrument'),
      value:
        record.reason === 'issuer_name_not_found'
          ? 'Card/Account Unmatched in Settings'
          : record.reason.includes('missing_instrument')
            ? 'No Card/Account Identifier in Email'
            : 'Instrument Signal Matched',
    },
  ];

  // Failure Reason Details & Action Tips
  let reasonDescription = 'An unexpected issue occurred while processing this message.';
  let actionTip = 'Review the extracted data below and complete any missing fields before saving.';

  if (record.reason === 'extraction_failed') {
    reasonDescription = "Dinero's rule engine could not reliably parse the minimum required transaction details (amount, merchant, or date) from this message text.";
    actionTip = "Fill in the missing fields manually using the extracted data form below or Quick-Fill buttons from the email evidence.";
  } else if (record.reason === 'issuer_name_not_found') {
    reasonDescription = 'Transaction details were parsed successfully, but Dinero could not match the bank or card mentioned in the email to any configured payment instrument in your settings.';
    actionTip = 'Select an existing instrument from the dropdown below, or go to Settings → Payment Instruments to add this new card/account.';
  } else if (record.reason === 'pending_llm_enrichment') {
    reasonDescription = 'Rule-based parsers could not confidently extract all details. An AI LLM enrichment task has been enqueued to extract missing fields.';
    actionTip = 'You can wait for the background AI worker to complete or manually input the fields below and save.';
  } else if (record.reason === 'gate3_failed:missing_amount') {
    reasonDescription = 'Merchant name and date were identified, but no valid transaction amount figure could be parsed from the email body.';
    actionTip = 'Look at the email text in Source Email Evidence below and enter the amount in the form.';
  } else if (record.reason === 'gate3_failed:missing_counterparty') {
    reasonDescription = 'Transaction amount and date were found, but no counterparty or merchant name could be extracted, or candidate was rejected as generic boilerplate.';
    actionTip = 'Type the merchant name into the Merchant field below or use Quick-Fill.';
  } else if (record.reason === 'gate3_failed:missing_instrument') {
    reasonDescription = 'Amount, merchant, and date were extracted, but no bank name, card last-4 digits, or UPI ID signal was present in the email.';
    actionTip = 'Select the payment instrument used for this transaction from the Instrument dropdown below.';
  }

  // Confidence & Engine calculations
  let confidenceLabel = '0% (Unextracted)';
  let confidenceBadgeStyle = 'bg-red-500/10 text-red-700 border-red-200';
  let engineLabel = 'Standard Ladder';

  if (record.extraction_method?.startsWith('regex_')) {
    confidenceLabel = '100% (Rule Match)';
    confidenceBadgeStyle = 'bg-emerald-500/10 text-emerald-800 border-emerald-300';
    engineLabel = `Deterministic (${record.extraction_method})`;
  } else if (record.extraction_method === 'llm_layer6') {
    engineLabel = 'AI Layer 6 LLM';
    if (record.confidence_score != null) {
      const pct = Math.round(record.confidence_score * 100);
      confidenceLabel = `${pct}% Confidence`;
      if (pct >= 80) {
        confidenceBadgeStyle = 'bg-emerald-500/10 text-emerald-800 border-emerald-300';
      } else if (pct >= 50) {
        confidenceBadgeStyle = 'bg-amber-500/10 text-amber-800 border-amber-300';
      }
    } else {
      confidenceLabel = 'AI Extracted (Medium)';
      confidenceBadgeStyle = 'bg-amber-500/10 text-amber-800 border-amber-300';
    }
  } else if (record.reason === 'pending_llm_enrichment') {
    engineLabel = 'AI Layer 6 LLM (Pending)';
    confidenceLabel = 'Enqueued for AI';
    confidenceBadgeStyle = 'bg-amber-500/10 text-amber-800 border-amber-300';
  } else if (record.confidence_score != null) {
    const pct = Math.round(record.confidence_score * 100);
    confidenceLabel = `${pct}% Confidence`;
    if (pct >= 80) {
      confidenceBadgeStyle = 'bg-emerald-500/10 text-emerald-800 border-emerald-300';
    } else {
      confidenceBadgeStyle = 'bg-amber-500/10 text-amber-800 border-amber-300';
    }
  }

  return (
    <aside
      className={asideClasses}
      role="complementary"
      aria-label="Unassigned detail"
      aria-hidden={!isOpen}
      style={asideStyle}
    >
      {/* Header */}
      <div
        className={cn('flex items-start justify-between p-5 flex-shrink-0', inline && 'pt-0')}
        style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
      >
        <div className="min-w-0 flex-1 pr-3">
          <span className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm mb-2 inline-block uppercase tracking-wider bg-red-500/10 text-red-700">
            Action Required
          </span>
          <p className="text-[15px] font-semibold text-[#064E3B]">{title}</p>
        </div>

        <button
          type="button"
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B] flex-shrink-0"
          onClick={onClose}
          aria-label="Close inspector"
        >
          <X className="w-5 h-5" />
        </button>
      </div>

      {/* Content */}
      <div
        className={cn(
          'flex-1 overflow-y-auto flex flex-col',
          inline ? 'py-6 px-4 md:px-8 max-w-5xl mx-auto w-full' : 'p-4'
        )}
      >
        <div className="flex-1 space-y-6">
          {/* Why it failed & Diagnostic Breakdown */}
          <div className="bg-white/70 rounded-xl p-5 border border-red-200/80 shadow-sm space-y-4">
            <div className="flex items-center justify-between flex-wrap gap-2 pb-3 border-b border-red-100">
              <h3 className="text-sm font-semibold text-red-900 flex items-center gap-2">
                <AlertTriangle className="w-4 h-4 text-red-600" /> Why did this fail?
              </h3>

              {/* Confidence & Engine badges */}
              <div className="flex items-center gap-2 flex-wrap">
                <span className="text-[11px] font-medium px-2 py-0.5 rounded-full bg-[#064E3B]/10 text-[#064E3B]">
                  {engineLabel}
                </span>
                <span
                  className={cn(
                    'text-[11px] font-bold px-2 py-0.5 rounded-full border',
                    confidenceBadgeStyle
                  )}
                >
                  {confidenceLabel}
                </span>
              </div>
            </div>

            <p className="text-[13px] text-[#064E3B]/90 leading-relaxed font-medium">
              {reasonDescription}
            </p>

            {/* Diagnostic Pass / Fail Checklist */}
            <div className="bg-white/90 rounded-lg p-3 border border-[#064E3B]/10">
              <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 mb-2.5">
                Ingestion Diagnostic Checklist
              </h4>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-2 text-xs">
                {checks.map((check) => (
                  <div
                    key={check.id}
                    className={cn(
                      'flex items-start gap-2 p-2 rounded-md border',
                      check.passed
                        ? 'bg-emerald-50/60 border-emerald-200/60 text-emerald-900'
                        : 'bg-red-50/60 border-red-200/60 text-red-900'
                    )}
                  >
                    {check.passed ? (
                      <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 flex-shrink-0 mt-0.5" />
                    ) : (
                      <XCircle className="w-3.5 h-3.5 text-red-600 flex-shrink-0 mt-0.5" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="font-semibold text-[11px] flex items-center justify-between gap-1">
                        <span>{check.label}</span>
                        <span
                          className={cn(
                            'text-[9px] font-extrabold uppercase px-1 rounded',
                            check.passed ? 'bg-emerald-200/60 text-emerald-900' : 'bg-red-200/60 text-red-900'
                          )}
                        >
                          {check.passed ? 'Passed' : 'Failed'}
                        </span>
                      </div>
                      <p className="text-[11px] opacity-80 truncate">{check.value}</p>
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* Context Actionable Tip */}
            <div className="bg-red-50/60 rounded-lg p-3 text-xs text-red-900/90 font-medium border border-red-100">
              <strong>Tip:</strong> {actionTip}
            </div>
          </div>

          {/* Extracted Data Form */}
          <div>
            <h3 className="text-sm font-semibold mb-3 text-[#064E3B] flex items-center gap-2">
              <FileText className="w-4 h-4" /> Extracted Data
            </h3>
            <div className="bg-white rounded-xl border border-[#064E3B]/10 overflow-hidden text-[13px]">
              
              {/* Row 1: Merchant (Full width - 1 column) */}
              <div className="grid grid-cols-[110px_1fr] border-b border-[#064E3B]/5">
                <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                  Merchant
                </div>
                <div className="px-4 py-2 flex items-center">
                  <Input
                    id="save-txn-merchant"
                    value={merchant}
                    onChange={(e) => setMerchant(e.target.value)}
                    className="h-8 bg-white/50 focus-visible:bg-white text-[13px]"
                    placeholder="Enter merchant name"
                  />
                </div>
              </div>

              {/* Row 2: Amount & Direction (2 columns) */}
              <div className="grid grid-cols-1 md:grid-cols-2 border-b border-[#064E3B]/5">
                <div className="grid grid-cols-[110px_1fr] border-b md:border-b-0 md:border-r border-[#064E3B]/5">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Amount
                  </div>
                  <div className="px-4 py-2 flex items-center">
                    <Input
                      id="save-txn-amount"
                      type="number"
                      min="0"
                      step="0.01"
                      value={amount}
                      onChange={(e) => setAmount(e.target.value)}
                      className="h-8 bg-white/50 focus-visible:bg-white font-mono text-[13px]"
                      placeholder="0.00"
                    />
                  </div>
                </div>

                <div className="grid grid-cols-[110px_1fr]">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Direction
                  </div>
                  <div className="px-4 py-2 flex items-center">
                    <Select value={direction} onValueChange={(v) => setDirection(v as 'debit' | 'credit')}>
                      <SelectTrigger aria-label="Direction" className="h-8 bg-white/50 focus:bg-white text-[13px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="debit">Debit (spend)</SelectItem>
                        <SelectItem value="credit">Credit (income)</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </div>

              {/* Row 3: Date & Instrument (2 columns) */}
              <div className="grid grid-cols-1 md:grid-cols-2 border-b border-[#064E3B]/5">
                <div className="grid grid-cols-[110px_1fr] border-b md:border-b-0 md:border-r border-[#064E3B]/5">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Date
                  </div>
                  <div className="px-4 py-2 flex items-center">
                    <DatePicker
                      id="save-txn-date"
                      size="sm"
                      value={date}
                      onChange={(val) => setDate(val)}
                    />
                  </div>
                </div>

                <div className="grid grid-cols-[110px_1fr]">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Instrument
                  </div>
                  <div className="px-4 py-2 flex items-center">
                    <InstrumentPicker
                      value={instrumentId}
                      onChange={setInstrumentId}
                      instruments={instruments}
                      triggerClassName="h-8 bg-white/50 focus:bg-white text-[13px]"
                    />
                  </div>
                </div>
              </div>

              {/* Row 4: Ref ID & Source ID (2 columns) */}
              <div className="grid grid-cols-1 md:grid-cols-2">
                <div className="grid grid-cols-[110px_1fr] border-b md:border-b-0 md:border-r border-[#064E3B]/5">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Ref ID
                  </div>
                  <div className="px-4 py-2 flex items-center">
                    <Input
                      id="save-txn-ref"
                      value={referenceId}
                      onChange={(e) => setReferenceId(e.target.value)}
                      className="h-8 bg-white/50 focus-visible:bg-white text-[13px]"
                      placeholder="Optional reference"
                    />
                  </div>
                </div>

                <div className="grid grid-cols-[110px_1fr]">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                    Source ID
                  </div>
                  <div
                    className="px-4 py-3 text-[#064E3B]/70 font-mono text-[11px] truncate flex items-center"
                    title={record.source_message_id ?? undefined}
                  >
                    {record.source_message_id ?? 'N/A'}
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* Source Email Section (Dedicated Card) */}
          {(emailHtml || emailText || record.body_snippet) && (
            <div>
              <h3 className="text-sm font-semibold mb-3 text-[#064E3B] flex items-center gap-2">
                <FileText className="w-4 h-4" /> Source Email Evidence
              </h3>
              <GmailEmailViewer
                html={emailHtml || undefined}
                text={emailText || record.body_snippet || undefined}
                sender={emailSender || record.merchant_raw || 'Bank Alert'}
                subject={emailSubject}
                date={record.event_time}
                maxHeight="420px"
                onQuickFill={({ field, value }) => {
                  if (field === 'amount') setAmount(value);
                  if (field === 'merchant') setMerchant(value);
                  if (field === 'date') setDate(value);
                  if (field === 'referenceId') setReferenceId(value);
                  toast({
                    title: 'Quick-Fill Applied',
                    description: `Updated ${field} to "${value}"`,
                  });
                }}
              />
            </div>
          )}
        </div>

        {/* Action Zone at bottom */}
        <div className="mt-8 pt-4 border-t border-[#064E3B]/10 flex justify-end gap-3">
          <Button
            variant="outline"
            className="text-[13px] h-9 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5 font-medium"
            onClick={handleDismiss}
            disabled={resolveManually.isPending}
          >
            <Ban className="w-3.5 h-3.5 mr-2" /> Not a Transaction
          </Button>
          <Button
            className="text-[13px] h-9 font-medium bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
            onClick={handleSave}
            disabled={!canSubmit || resolveManually.isPending}
          >
            <Save className="w-3.5 h-3.5 mr-2" /> Save as Transaction
          </Button>
        </div>
      </div>
    </aside>
  );
}
