import { useState, useEffect, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  X,
  Loader2,
  Save,
  Trash2,
  KeyRound,
  ExternalLink,
  CheckCircle2,
  Copy,
  Check,
  Search,
  FileText,
  Upload,
  PieChart,
  Calendar,
  CreditCard,
  Building,
  ShieldCheck,
  ArrowUpRight,
  ArrowDownRight,
} from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { useToast } from '@/hooks/use-toast';
import { confirmAction } from '@/lib/confirmDialog';
import { queryKeys } from '@/lib/queryKeys';
import { useInstrumentForm } from './useInstrumentForm';
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
import InstrumentCardHero from './InstrumentCardHero';
import InstrumentAnalyticsTab from './InstrumentAnalyticsTab';
import TransactionItemCard from '@/components/transactions/TransactionItemCard';
import CardNumberInput from '@/components/ui/CardNumberInput';
import type { InstrumentRecord } from '@/lib/ipc';

type Tab = 'details' | 'transactions' | 'statements' | 'analytics';

interface InstrumentInspectorProps {
  instrument: InstrumentRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

export default function InstrumentInspector({
  instrument,
  onClose,
  inline = false,
}: InstrumentInspectorProps) {
  const navigate = useNavigate();
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const isOpen = !!instrument;

  const [activeTab, setActiveTab] = useState<Tab>('details');
  const [txSearchQuery, setTxSearchQuery] = useState('');
  const [copiedId, setCopiedId] = useState(false);

  const {
    inst,
    isLoading,
    detailInst,
    forgetPassword,
    issuerName,
    setIssuerName,
    maskedIdentifier,
    setMaskedIdentifier,
    nickname,
    setNickname,
    fullIdentifier,
    setFullIdentifier,
    billingCycleDay,
    setBillingCycleDay,
    bankIfsc,
    setBankIfsc,
    instrumentType,
    setInstrumentType,
    status,
    setStatus,
    creditLimit,
    setCreditLimit,
    network,
    setNetwork,
    accountType,
    setAccountType,
    upiVpa,
    setUpiVpa,
    rewardsSummary,
    setRewardsSummary,
    statementDueDate,
    setStatementDueDate,
    minimumDue,
    setMinimumDue,
    isSaving,
    isDeleting,
    showSavedConfirm,
    recentTransactions,
    totalTxCount,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isTxLoading,
    instrumentStatements,
    instrumentPasswords,
    handleSave,
    handleDelete,
  } = useInstrumentForm(instrument?.id, instrument, onClose);

  // Reset tab when instrument changes
  useEffect(() => {
    setActiveTab('details');
    setTxSearchQuery('');
  }, [instrument?.id]);

  const filteredTransactions = useMemo(() => {
    if (!txSearchQuery.trim()) return recentTransactions;
    const q = txSearchQuery.toLowerCase().trim();
    return recentTransactions.filter(
      (tx) =>
        tx.merchant.toLowerCase().includes(q) ||
        (tx.category && tx.category.toLowerCase().includes(q)) ||
        tx.amount.toString().includes(q)
    );
  }, [recentTransactions, txSearchQuery]);

  const handleCopyAccountId = () => {
    if (!inst?.id) return;
    navigator.clipboard.writeText(inst.id);
    setCopiedId(true);
    toast({
      title: 'Account ID Copied',
      description: `Copied ${inst.id} to clipboard.`,
    });
    setTimeout(() => setCopiedId(false), 2000);
  };

  if (!inst) return null;

  const TABS: { id: Tab; label: string; icon?: React.ReactNode }[] = [
    { id: 'details', label: 'Details' },
    { id: 'transactions', label: 'Transactions' },
    { id: 'statements', label: 'Statements' },
    { id: 'analytics', label: 'Analytics' },
  ];

  return (
    <aside
      className={cn(
        !inline && 'inspector-panel',
        !inline && !isOpen && 'closed',
        inline && 'w-full h-full flex flex-col',
        !inline && 'flex-shrink-0'
      )}
      role="complementary"
      aria-label="Account detail"
      aria-hidden={!isOpen}
      style={
        inline ? { backgroundColor: '#F8E7C9' } : { width: isOpen ? 'var(--inspector-width)' : 0 }
      }
    >
      {/* Header Bar */}
      <div
        className={cn('flex items-center justify-between p-5 flex-shrink-0', inline && 'pt-4')}
        style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
      >
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-9 h-9 rounded-xl bg-[#064E3B] text-[#F8E7C9] flex items-center justify-center text-sm font-bold shrink-0">
            {inst.issuer_name.charAt(0).toUpperCase()}
          </div>
          <div className="min-w-0">
            <h2 className="text-[15px] font-bold text-[#064E3B] truncate">{inst.issuer_name}</h2>
            <p className="text-[11px] font-mono text-[#064E3B]/60 truncate">••{inst.masked_identifier}</p>
          </div>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B] cursor-pointer"
            onClick={() => navigate(`/instruments/${inst.id}`)}
            aria-label="Open full page"
            title="Open full page"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B] cursor-pointer"
            onClick={onClose}
            aria-label="Close inspector"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* Tabs Bar */}
      <div
        className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1.5 border-b border-[#064E3B]/10 overflow-x-auto bg-[#F8E7C9]/40"
        role="tablist"
      >
        {TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            className={cn(
              'flex items-center gap-1.5 px-3.5 py-1.5 text-[12px] font-bold rounded-full transition-all whitespace-nowrap cursor-pointer',
              activeTab === tab.id
                ? 'bg-[#064E3B] text-[#F8E7C9] shadow-xs'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10 hover:text-[#064E3B]'
            )}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
            {tab.id === 'transactions' && (totalTxCount > 0 || recentTransactions.length > 0) && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  activeTab === tab.id ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {totalTxCount || recentTransactions.length}
              </span>
            )}
            {tab.id === 'statements' && instrumentStatements.length > 0 && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  activeTab === tab.id ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {instrumentStatements.length}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Content Scroll Area */}
      <div className={cn('flex-1 overflow-y-auto', inline ? 'p-6 max-w-4xl mx-auto w-full' : 'p-4')}>
        {isLoading && !detailInst ? (
          <div className="flex items-center justify-center py-16">
            <Loader2 className="w-6 h-6 animate-spin text-[#064E3B]" />
          </div>
        ) : (
          <div className="space-y-6">
            {/* Digital Card Preview Hero */}
            <InstrumentCardHero instrument={inst} />

            {/* DETAILS TAB */}
            {activeTab === 'details' && (
              <div className="space-y-5 animate-in fade-in-50 duration-200">
                {/* GRID SECTION 1 & 2 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {/* Account Specifications & Identity Card */}
                  <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-3.5 shadow-xs">
                    <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 border-b border-[#064E3B]/10 pb-2 flex items-center justify-between">
                      <span className="flex items-center gap-1.5">
                        <CreditCard className="w-3.5 h-3.5 text-[#064E3B]" /> Identity & Specifications
                      </span>
                      <span className="text-[10px] font-mono text-[#064E3B]/50">ID & Type</span>
                    </h4>

                    <div className="space-y-3">
                      {/* Issuer / Institution Name */}
                      <div className="space-y-1">
                        <Label htmlFor="insp-issuer-name" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Issuer / Institution Name
                        </Label>
                        <Input
                          id="insp-issuer-name"
                          value={issuerName}
                          onChange={(e) => setIssuerName(e.target.value)}
                          placeholder="e.g. SBI Card, Axis Bank, HDFC Bank"
                          className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                        />
                      </div>

                      {/* Display Nickname */}
                      <div className="space-y-1">
                        <Label htmlFor="insp-nickname" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Display Nickname
                        </Label>
                        <Input
                          id="insp-nickname"
                          value={nickname}
                          onChange={(e) => setNickname(e.target.value)}
                          placeholder="e.g. Primary Spender, Travel Card"
                          className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                        />
                      </div>

                      {/* Masked Identifier / Card Tail */}
                      <div className="space-y-1">
                        <Label htmlFor="insp-masked-id" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Masked Identifier / Tail
                        </Label>
                        <Input
                          id="insp-masked-id"
                          value={maskedIdentifier}
                          onChange={(e) => setMaskedIdentifier(e.target.value)}
                          placeholder="e.g. 7603, user@okaxis"
                          className="h-9 text-[13px] font-mono font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                        />
                      </div>

                      {/* Instrument Type */}
                      <div className="space-y-1">
                        <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Instrument Type
                        </Label>
                        <Select value={instrumentType} onValueChange={setInstrumentType}>
                          <SelectTrigger className="h-9 text-[13px] font-bold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl">
                            <SelectValue placeholder="Select type" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="credit_card">Credit Card</SelectItem>
                            <SelectItem value="bank_account">Bank Account</SelectItem>
                            <SelectItem value="upi_vpa">UPI VPA</SelectItem>
                            <SelectItem value="debit_card">Debit Card</SelectItem>
                            <SelectItem value="wallet">Wallet</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>

                      {/* Status */}
                      <div className="space-y-1">
                        <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Status
                        </Label>
                        <Select value={status} onValueChange={setStatus}>
                          <SelectTrigger className="h-9 text-[13px] font-bold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl">
                            <SelectValue placeholder="Select status" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="active">Active</SelectItem>
                            <SelectItem value="inactive">Inactive</SelectItem>
                            <SelectItem value="archived">Archived</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>

                      {/* Account ID Copyable */}
                      <div className="flex justify-between items-center pt-2 border-t border-[#064E3B]/10">
                        <span className="text-[#064E3B]/70 text-[12px] font-medium">Account ID</span>
                        <div className="flex items-center gap-1">
                          <span className="font-mono text-[11px] text-[#064E3B]/80 truncate max-w-[130px]" title={inst.id}>
                            {inst.id}
                          </span>
                          <button
                            type="button"
                            onClick={handleCopyAccountId}
                            className="p-1 rounded text-[#064E3B]/60 hover:text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors"
                            title="Copy ID"
                          >
                            {copiedId ? <Check className="w-3 h-3 text-emerald-600" /> : <Copy className="w-3 h-3" />}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* Account & Security Configuration Card */}
                  <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-3.5 shadow-xs">
                    <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 border-b border-[#064E3B]/10 pb-2 flex items-center justify-between">
                      <span className="flex items-center gap-1.5">
                        <Building className="w-3.5 h-3.5 text-[#064E3B]" /> Security & Configuration
                      </span>
                      <span className="text-[10px] font-mono text-[#064E3B]/50">Credentials</span>
                    </h4>

                    <div className="space-y-3">
                      {/* Full Number */}
                      <div className="space-y-1">
                        <Label
                          htmlFor="insp-inst-full-id"
                          className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70"
                        >
                          Full Card / Account / VPA Number
                        </Label>
                        <CardNumberInput
                          id="insp-inst-full-id"
                          value={fullIdentifier}
                          onChange={setFullIdentifier}
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                          placeholder="4532 7603 1920 8841"
                        />
                      </div>

                      {/* Card Network (for credit/debit cards) */}
                      {(instrumentType === 'credit_card' || instrumentType === 'debit_card') && (
                        <div className="space-y-1">
                          <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                            Card Network
                          </Label>
                          <Select value={network || 'Visa'} onValueChange={setNetwork}>
                            <SelectTrigger className="h-9 text-[13px] font-bold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl">
                              <SelectValue placeholder="Select network" />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="Visa">Visa</SelectItem>
                              <SelectItem value="Mastercard">Mastercard</SelectItem>
                              <SelectItem value="RuPay">RuPay</SelectItem>
                              <SelectItem value="Amex">American Express</SelectItem>
                              <SelectItem value="Diners">Diners Club</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      )}

                      {/* Account Subtype (for bank accounts) */}
                      {instrumentType === 'bank_account' && (
                        <div className="space-y-1">
                          <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                            Account Subtype
                          </Label>
                          <Select value={accountType || 'Savings'} onValueChange={setAccountType}>
                            <SelectTrigger className="h-9 text-[13px] font-bold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl">
                              <SelectValue placeholder="Select account type" />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="Savings">Savings Account</SelectItem>
                              <SelectItem value="Current">Current Account</SelectItem>
                              <SelectItem value="Salary">Salary Account</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      )}

                      {/* Associated UPI VPA */}
                      <div className="space-y-1">
                        <Label htmlFor="insp-vpa" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Associated UPI VPA
                        </Label>
                        <Input
                          id="insp-vpa"
                          value={upiVpa}
                          onChange={(e) => setUpiVpa(e.target.value)}
                          placeholder="e.g. user@okaxis, 9876543210@upi"
                          className="h-9 text-[13px] font-mono bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                        />
                      </div>
                    </div>
                  </div>
                </div>

                {/* GRID SECTION 3 & 4 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {/* Billing & Credit Specifications Card */}
                  <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-3.5 shadow-xs">
                    <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 border-b border-[#064E3B]/10 pb-2 flex items-center justify-between">
                      <span className="flex items-center gap-1.5">
                        <ShieldCheck className="w-3.5 h-3.5 text-[#064E3B]" /> Billing & Limits
                      </span>
                      <span className="text-[10px] font-mono text-[#064E3B]/50">Cycle & Limit</span>
                    </h4>

                    <div className="space-y-3">
                      {instrumentType === 'credit_card' && (
                        <>
                          <div className="space-y-1">
                            <Label
                              htmlFor="insp-inst-billing"
                              className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70"
                            >
                              Billing Cycle Day (1-31)
                            </Label>
                            <Input
                              id="insp-inst-billing"
                              type="number"
                              min="1"
                              max="31"
                              value={billingCycleDay}
                              onChange={(e) => setBillingCycleDay(e.target.value)}
                              placeholder="e.g. 15"
                              className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                              onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                            />
                            {billingCycleDay && (
                              <p className="text-[10px] text-[#064E3B]/70 italic pt-0.5">
                                Statements generated on the {billingCycleDay}th of every month.
                              </p>
                            )}
                          </div>

                          <div className="space-y-1">
                            <Label
                              htmlFor="insp-inst-limit"
                              className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70"
                            >
                              Credit Limit (₹)
                            </Label>
                            <Input
                              id="insp-inst-limit"
                              type="number"
                              value={creditLimit}
                              onChange={(e) => setCreditLimit(e.target.value)}
                              placeholder="e.g. 150000"
                              className="h-9 text-[13px] font-semibold font-mono bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                              onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                            />
                            {creditLimit && parseFloat(creditLimit) > 0 && (
                              <div className="space-y-1 pt-1">
                                <div className="flex justify-between text-[10px] font-mono text-[#064E3B]/70">
                                  <span>Utilization</span>
                                  <span>
                                    {Math.min(100, Math.max(0, (((inst.current_balance ?? 0) / parseFloat(creditLimit)) * 100))).toFixed(1)}% Used
                                  </span>
                                </div>
                                <div className="h-1.5 w-full bg-[#064E3B]/10 rounded-full overflow-hidden">
                                  <div
                                    className="h-full bg-[#064E3B] transition-all rounded-full"
                                    style={{
                                      width: `${Math.min(100, Math.max(0, (((inst.current_balance ?? 0) / parseFloat(creditLimit)) * 100)))}%`,
                                    }}
                                  />
                                </div>
                              </div>
                            )}
                          </div>
                        </>
                      )}

                      {instrumentType === 'bank_account' && (
                        <div className="space-y-1">
                          <Label
                            htmlFor="insp-inst-ifsc"
                            className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70"
                          >
                            Bank IFSC Code
                          </Label>
                          <Input
                            id="insp-inst-ifsc"
                            value={bankIfsc}
                            onChange={(e) => setBankIfsc(e.target.value)}
                            placeholder="e.g. HDFC0000123"
                            className="h-9 text-[13px] uppercase font-mono font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                          />
                        </div>
                      )}
                    </div>
                  </div>

                  {/* Extracted Metadata & Rewards Card */}
                  <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-3.5 shadow-xs">
                    <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 border-b border-[#064E3B]/10 pb-2 flex items-center justify-between">
                      <span className="flex items-center gap-1.5">
                        <FileText className="w-3.5 h-3.5 text-[#064E3B]" /> Statement Metadata & Rewards
                      </span>
                      <span className="text-[10px] font-mono text-[#064E3B]/50">Extracted & Editable</span>
                    </h4>

                    <div className="space-y-3">
                      {/* Statement Due Date & Minimum Due Editable Controls */}
                      <div className="grid grid-cols-2 gap-2 text-[12px]">
                        <div className="space-y-1">
                          <Label htmlFor="insp-due-date" className="text-[10px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                            Latest Bill Due Date
                          </Label>
                          <DatePicker
                            id="insp-due-date"
                            value={statementDueDate}
                            onChange={setStatementDueDate}
                            placeholder="Select due date"
                            triggerClassName="h-9 text-[12px] font-mono font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl w-full"
                          />
                        </div>
                        <div className="space-y-1">
                          <Label htmlFor="insp-min-due" className="text-[10px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                            Minimum Amount Due (₹)
                          </Label>
                          <Input
                            id="insp-min-due"
                            type="number"
                            step="0.01"
                            value={minimumDue}
                            onChange={(e) => setMinimumDue(e.target.value)}
                            placeholder="e.g. 1200.00"
                            className="h-9 text-[12px] font-mono font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                          />
                        </div>
                      </div>

                      {/* Rewards Summary */}
                      <div className="space-y-1">
                        <Label htmlFor="insp-rewards" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Rewards & Cashback Summary
                        </Label>
                        <Input
                          id="insp-rewards"
                          value={rewardsSummary}
                          onChange={(e) => setRewardsSummary(e.target.value)}
                          placeholder="e.g. 1,250 EDGE Points • 1.5% Unlimited Cashback"
                          className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
                          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                        />
                      </div>
                    </div>
                  </div>
                </div>

                {/* Password Vault Entries */}
                {instrumentPasswords.length > 0 && (
                  <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-2 shadow-xs">
                    <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 flex items-center gap-1.5">
                      <ShieldCheck className="w-3.5 h-3.5 text-[#064E3B]" /> Saved Statement Passwords
                    </h4>
                    {instrumentPasswords.map((p) => (
                      <div
                        key={p.id}
                        className="flex items-center justify-between p-2.5 rounded-xl border border-[#064E3B]/10 bg-[#F3EBDD]/70"
                      >
                        <div className="flex items-center gap-2">
                          <KeyRound className="w-3.5 h-3.5 text-[#064E3B]" />
                          <span className="text-[13px] text-[#064E3B] font-semibold">
                            Password vault entry (Used {p.success_count}x)
                          </span>
                        </div>
                        <button
                          type="button"
                          onClick={() => forgetPassword.mutate(p.id)}
                          disabled={forgetPassword.isPending}
                          className="text-xs font-semibold px-2.5 py-1 rounded-lg transition-colors hover:bg-red-50 text-red-600 cursor-pointer"
                        >
                          Forget
                        </button>
                      </div>
                    ))}
                  </div>
                )}

                {showSavedConfirm && (
                  <p className="flex items-center gap-1.5 text-xs font-bold text-emerald-600">
                    <CheckCircle2 className="w-3.5 h-3.5" /> Changes saved successfully.
                  </p>
                )}

                {/* Save / Delete Action Bar */}
                <div className="flex items-center gap-3 pt-2">
                  <button
                    type="button"
                    onClick={handleSave}
                    disabled={isSaving}
                    className="flex-1 h-10 rounded-xl text-[13px] font-bold flex items-center justify-center gap-2 transition-all bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-sm cursor-pointer"
                  >
                    {isSaving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
                    Save Changes
                  </button>
                  <button
                    type="button"
                    onClick={handleDelete}
                    disabled={isDeleting}
                    className="h-10 px-4 rounded-xl text-[13px] font-bold flex items-center justify-center gap-2 transition-colors border border-red-500/30 text-red-700 hover:bg-red-50 cursor-pointer"
                  >
                    {isDeleting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Trash2 className="w-4 h-4" />}
                    Delete Account
                  </button>
                </div>
              </div>
            )}

            {/* TRANSACTIONS TAB */}
            {activeTab === 'transactions' && (
              <div className="space-y-4 animate-in fade-in-50 duration-200">
                {/* Search & Header bar */}
                <div className="flex flex-col md:flex-row md:items-center justify-between gap-2.5">
                  <div className="relative flex-1">
                    <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-[#064E3B]/50" />
                    <input
                      type="text"
                      placeholder="Search transactions for this card..."
                      value={txSearchQuery}
                      onChange={(e) => setTxSearchQuery(e.target.value)}
                      className="w-full h-8 pl-8 pr-7 text-[12px] bg-[#F3EBDD]/80 border border-[#064E3B]/15 rounded-xl outline-none text-[#064E3B] placeholder:text-[#064E3B]/40 focus:border-[#064E3B]/40"
                    />
                    {txSearchQuery && (
                      <button
                        type="button"
                        onClick={() => setTxSearchQuery('')}
                        className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
                      >
                        <X className="w-3 h-3" />
                      </button>
                    )}
                  </div>

                  <button
                    type="button"
                    onClick={() => navigate(`/transactions?instrument=${inst.id}`)}
                    className="text-xs font-bold text-[#064E3B] hover:underline flex items-center gap-1 cursor-pointer shrink-0"
                  >
                    View All Ledger ↗
                  </button>
                </div>

                {filteredTransactions.length === 0 ? (
                  <div className="text-center py-12 bg-[#F8E7C9]/40 rounded-2xl border border-[#064E3B]/10">
                    <p className="text-xs text-[#064E3B]/60 italic">
                      {txSearchQuery ? 'No matching transactions found.' : 'No transactions found for this account.'}
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2.5">
                    {filteredTransactions.map((tx) => (
                      <TransactionItemCard
                        key={tx.id}
                        transaction={tx}
                        onClick={() => navigate(`/transactions/${tx.id}`)}
                      />
                    ))}

                    {/* Pagination / Load More */}
                    {hasNextPage && !txSearchQuery && (
                      <div className="pt-2 text-center">
                        <button
                          type="button"
                          onClick={() => fetchNextPage()}
                          disabled={isFetchingNextPage}
                          className="w-full py-2.5 px-4 rounded-xl bg-[#064E3B]/10 hover:bg-[#064E3B]/20 text-[#064E3B] font-bold text-xs transition-colors flex items-center justify-center gap-2 cursor-pointer disabled:opacity-50"
                        >
                          {isFetchingNextPage ? (
                            <>
                              <Loader2 className="w-3.5 h-3.5 animate-spin" />
                              <span>Loading transactions...</span>
                            </>
                          ) : (
                            <span>Load More Transactions ({filteredTransactions.length} of {totalTxCount})</span>
                          )}
                        </button>
                      </div>
                    )}

                    <div className="text-center text-[11px] font-mono text-[#064E3B]/60 pt-1">
                      Showing {filteredTransactions.length} of {totalTxCount} transactions
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* STATEMENTS TAB */}
            {activeTab === 'statements' && (
              <div className="space-y-4 animate-in fade-in-50 duration-200">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-bold uppercase tracking-wider text-[#064E3B]/70 flex items-center gap-1.5">
                    <FileText className="w-3.5 h-3.5" /> Uploaded Statements ({instrumentStatements.length})
                  </span>
                  <button
                    type="button"
                    onClick={() => navigate('/statements')}
                    className="text-xs font-bold px-3 py-1.5 rounded-xl bg-[#064E3B] text-[#F8E7C9] flex items-center gap-1.5 hover:bg-[#064E3B]/90 cursor-pointer shadow-xs"
                  >
                    <Upload className="w-3.5 h-3.5" /> Upload Statement
                  </button>
                </div>

                {instrumentStatements.length === 0 ? (
                  <div className="text-center py-12 bg-[#F8E7C9]/40 rounded-2xl border border-[#064E3B]/10 space-y-3">
                    <FileText className="w-8 h-8 text-[#064E3B]/40 mx-auto" />
                    <p className="text-xs text-[#064E3B]/60 italic">No statements uploaded yet for this account.</p>
                    <button
                      type="button"
                      onClick={() => navigate('/statements')}
                      className="text-xs font-bold text-[#064E3B] underline cursor-pointer"
                    >
                      Go to Statements Center to import PDFs
                    </button>
                  </div>
                ) : (
                  <div className="space-y-2.5">
                    {instrumentStatements.map((s) => (
                      <div
                        key={s.id}
                        className="p-4 rounded-2xl bg-[#F8E7C9]/70 border border-[#064E3B]/10 space-y-2 shadow-xs"
                      >
                        <div className="flex items-start justify-between gap-2">
                          <span className="text-[13px] font-bold text-[#064E3B] truncate" title={s.file_name}>
                            {s.file_name}
                          </span>
                          <span
                            className={cn(
                              'text-[10px] font-extrabold px-2.5 py-0.5 rounded-full uppercase tracking-wider shrink-0',
                              s.status === 'PROCESSED'
                                ? 'bg-emerald-500/15 text-emerald-800 border border-emerald-500/20'
                                : 'bg-amber-500/15 text-amber-900 border border-amber-500/20'
                            )}
                          >
                            {s.status}
                          </span>
                        </div>
                        <div className="flex items-center justify-between text-[11px] text-[#064E3B]/70 font-medium pt-1 border-t border-[#064E3B]/10">
                          <span>Uploaded {formatCustomDate(s.date)}</span>
                          <button
                            type="button"
                            onClick={() => navigate('/statements')}
                            className="font-bold text-[#064E3B] hover:underline"
                          >
                            View details →
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* ANALYTICS TAB */}
            {activeTab === 'analytics' && (
              <InstrumentAnalyticsTab transactions={recentTransactions} />
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
