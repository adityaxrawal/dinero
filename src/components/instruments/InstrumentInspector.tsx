import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { X, Loader2, Save, Trash2, KeyRound, ExternalLink, CheckCircle2 } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { useToast } from '@/hooks/use-toast';
import { confirmDelete } from '@/lib/confirmDialog';
import { queryKeys } from '@/lib/queryKeys';
import { useInstrumentForm } from './useInstrumentForm';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import type { InstrumentRecord } from '@/lib/ipc';

type Tab = 'details' | 'transactions' | 'statements';

interface InstrumentInspectorProps {
  instrument: InstrumentRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

/**
 * Right-side inspector panel for instruments. Replaces InstrumentDetail route as primary UI.
 */
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

  const {
    inst,
    isLoading,
    detailInst,
    forgetPassword,
    fullIdentifier,
    setFullIdentifier,
    billingCycleDay,
    setBillingCycleDay,
    bankIfsc,
    setBankIfsc,
    isSaving,
    isDeleting,
    showSavedConfirm,
    isNegative,
    recentTransactions,
    instrumentStatements,
    instrumentPasswords,
    handleSave,
    handleDelete,
  } = useInstrumentForm(instrument?.id, instrument, onClose);

  // Reset tab when instrument changes
  useEffect(() => {
    setActiveTab('details');
  }, [instrument?.id]);

  if (!inst) return null;

  const TABS: { id: Tab; label: string }[] = [
    { id: 'details', label: 'Details' },
    { id: 'transactions', label: 'Transactions' },
    { id: 'statements', label: 'Statements' },
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
      {/* Header */}
      <div
        className={cn('flex items-start justify-between p-5 flex-shrink-0', inline && 'pt-0')}
        style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
      >
        <div className="min-w-0 flex-1 pr-3">
          <div className="flex items-center gap-3 mb-3">
            <div
              className="w-10 h-10 rounded-xl flex items-center justify-center text-[15px] font-bold flex-shrink-0"
              style={{ background: '#064E3B', color: '#F8E7C9' }}
              aria-hidden="true"
            >
              {inst.issuer_name.charAt(0).toUpperCase()}
            </div>
            <div className="min-w-0">
              <p className="text-[15px] font-semibold truncate text-[#064E3B]">
                {inst.issuer_name}
              </p>
              <p className="text-[12px] text-[#064E3B]/60">••{inst.masked_identifier}</p>
            </div>
          </div>
          <p
            className="text-3xl font-bold tracking-tight"
            style={{ color: isNegative ? '#064E3B' : '#059669' }}
          >
            {isNegative ? '−' : ''}₹
            {Math.abs(inst.current_balance ?? 0).toLocaleString(undefined, {
              minimumFractionDigits: 2,
            })}
          </p>
        </div>

        <div className="flex items-center gap-1 flex-shrink-0">
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
            onClick={() => navigate(`/instruments/${inst.id}`)}
            aria-label="Open full page"
            title="Open full page"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
            onClick={onClose}
            aria-label="Close inspector"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1 overflow-x-auto" role="tablist">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            className={cn(
              'px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap',
              activeTab === tab.id
                ? 'bg-[#064E3B] text-[#F8E7C9]'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10'
            )}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div
        className={cn('flex-1 overflow-y-auto', inline ? 'p-8 max-w-3xl mx-auto w-full' : 'p-4')}
      >
        {isLoading && !detailInst ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-5 h-5 animate-spin" style={{ color: '#064E3B' }} />
          </div>
        ) : (
          <>
            {/* DETAILS TAB */}
            {activeTab === 'details' && (
              <div className="space-y-4">
                {/* Read-only stats block */}
                <div className="bg-[#F8E7C9]/50 rounded-xl overflow-hidden border border-[#064E3B]/10">
                  <div className="flex items-center justify-between p-3 border-b border-[#064E3B]/5">
                    <span className="text-[13px] font-medium text-[#064E3B]">Type</span>
                    <span className="capitalize text-[13px] font-medium text-[#064E3B]/70">
                      {inst.instrument_type.replace('_', ' ')}
                    </span>
                  </div>
                  <div className="flex items-center justify-between p-3 border-b border-[#064E3B]/5">
                    <span className="text-[13px] font-medium text-[#064E3B]">Status</span>
                    <span className="capitalize text-[13px] font-medium text-[#064E3B]/70">
                      {inst.status}
                    </span>
                  </div>
                  {inst.credit_limit != null && (
                    <div className="flex items-center justify-between p-3 border-b border-[#064E3B]/5">
                      <span className="text-[13px] font-medium text-[#064E3B]">Credit Limit</span>
                      <span className="text-[13px] font-medium text-[#064E3B]/70">
                        ₹{inst.credit_limit.toLocaleString(undefined, { minimumFractionDigits: 2 })}
                      </span>
                    </div>
                  )}
                  <div className="flex items-center justify-between p-3">
                    <span className="text-[13px] font-medium text-[#064E3B]">ID</span>
                    <span className="text-[13px] font-mono text-[#064E3B]/70 truncate max-w-[150px]">
                      {inst.id}
                    </span>
                  </div>
                </div>

                {/* Edit Form */}
                <div className="bg-[#F8E7C9]/50 rounded-xl overflow-hidden border border-[#064E3B]/10">
                  <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                    <Label
                      htmlFor="insp-inst-full-id"
                      className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                    >
                      Full Identifier (e.g. Card No)
                    </Label>
                    <Input
                      id="insp-inst-full-id"
                      value={fullIdentifier}
                      onChange={(e) => setFullIdentifier(e.target.value)}
                      className="h-7 text-[13px] border-none shadow-none p-0 bg-transparent focus-visible:ring-0 text-[#064E3B]"
                      onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                    />
                  </div>

                  {inst.instrument_type === 'credit_card' && (
                    <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                      <Label
                        htmlFor="insp-inst-billing"
                        className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                      >
                        Billing Cycle Day
                      </Label>
                      <Input
                        id="insp-inst-billing"
                        type="number"
                        min="1"
                        max="31"
                        value={billingCycleDay}
                        onChange={(e) => setBillingCycleDay(e.target.value)}
                        className="h-7 text-[13px] border-none shadow-none p-0 bg-transparent focus-visible:ring-0 text-[#064E3B]"
                        onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                      />
                    </div>
                  )}

                  {inst.instrument_type === 'bank_account' && (
                    <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                      <Label
                        htmlFor="insp-inst-ifsc"
                        className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                      >
                        IFSC Code
                      </Label>
                      <Input
                        id="insp-inst-ifsc"
                        value={bankIfsc}
                        onChange={(e) => setBankIfsc(e.target.value)}
                        className="h-7 text-[13px] uppercase border-none shadow-none p-0 bg-transparent focus-visible:ring-0 text-[#064E3B]"
                        onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                      />
                    </div>
                  )}
                </div>

                {/* Passwords */}
                {instrumentPasswords.length > 0 && (
                  <div className="space-y-2 pt-2">
                    <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                      Saved Passwords
                    </p>
                    {instrumentPasswords.map((p) => (
                      <div
                        key={p.id}
                        className="flex items-center justify-between p-3 rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50"
                      >
                        <div className="flex items-center gap-2">
                          <KeyRound className="w-3.5 h-3.5 text-[#064E3B]" />
                          <span className="text-[13px] text-[#064E3B]/70 font-medium">
                            Used {p.success_count}x
                          </span>
                        </div>
                        <button
                          type="button"
                          onClick={() => forgetPassword.mutate(p.id)}
                          disabled={forgetPassword.isPending}
                          className="text-xs font-medium px-2 py-1 rounded-md transition-colors hover:bg-red-50"
                          style={{ color: '#ef4444' }}
                        >
                          Forget
                        </button>
                      </div>
                    ))}
                  </div>
                )}

                {showSavedConfirm && (
                  <p
                    className="flex items-center gap-1.5 text-xs font-medium"
                    style={{ color: '#10b981' }}
                  >
                    <CheckCircle2 className="w-3.5 h-3.5" /> Saved successfully.
                  </p>
                )}

                <div className="flex flex-col gap-2 pt-2">
                  <button
                    type="button"
                    onClick={handleSave}
                    disabled={isSaving}
                    className="w-full h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-[#064E3B]"
                    style={{ background: '#064E3B', color: '#F8E7C9' }}
                  >
                    {isSaving ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    ) : (
                      <Save className="w-3.5 h-3.5" />
                    )}
                    Save Changes
                  </button>
                  <button
                    type="button"
                    onClick={handleDelete}
                    disabled={isDeleting}
                    className="w-full h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors hover:bg-red-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-red-500"
                    style={{ color: '#dc2626' }}
                  >
                    {isDeleting ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    ) : (
                      <Trash2 className="w-3.5 h-3.5" />
                    )}
                    Delete Account
                  </button>
                </div>
              </div>
            )}

            {/* TRANSACTIONS TAB */}
            {activeTab === 'transactions' && (
              <div className="space-y-2">
                <div className="flex items-center justify-between mb-3">
                  <span className="text-xs font-medium" style={{ color: '#6b8a7f' }}>
                    Recent Activity
                  </span>
                  <button
                    type="button"
                    onClick={() => navigate(`/transactions?instrument=${inst.id}`)}
                    className="text-xs font-medium transition-colors hover:underline"
                    style={{ color: '#064E3B' }}
                  >
                    View All
                  </button>
                </div>
                {recentTransactions.length === 0 ? (
                  <p className="text-xs text-center py-6" style={{ color: '#6b8a7f' }}>
                    No transactions found.
                  </p>
                ) : (
                  <div className="space-y-0.5">
                    {recentTransactions.map((tx) => (
                      <div
                        key={tx.id}
                        className="flex items-center justify-between p-2 rounded-lg cursor-pointer transition-colors"
                        style={{ border: '1px solid transparent' }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.background = 'var(--bg-card-hover)';
                          e.currentTarget.style.borderColor = 'var(--border-color)';
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.background = 'transparent';
                          e.currentTarget.style.borderColor = 'transparent';
                        }}
                        onClick={() => navigate(`/transactions?instrument=${inst.id}`)}
                      >
                        <div className="min-w-0 pr-2">
                          <p
                            className="text-xs font-medium truncate"
                            style={{ color: 'var(--text-primary)' }}
                          >
                            {tx.merchant}
                          </p>
                          <p className="text-[10px]" style={{ color: '#6b8a7f' }}>
                            {formatCustomDate(tx.date)}
                          </p>
                        </div>
                        <span
                          className={cn(
                            'text-xs font-semibold amount flex-shrink-0',
                            tx.amount < 0 ? 'amount-debit' : 'amount-credit'
                          )}
                        >
                          {tx.amount < 0 ? '−' : '+'}₹{Math.abs(tx.amount).toLocaleString()}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* STATEMENTS TAB */}
            {activeTab === 'statements' && (
              <div className="space-y-3">
                {instrumentStatements.length === 0 ? (
                  <p className="text-xs text-center py-6" style={{ color: '#6b8a7f' }}>
                    No statements uploaded.
                  </p>
                ) : (
                  <div className="space-y-2">
                    {instrumentStatements.map((s) => (
                      <div
                        key={s.id}
                        className="p-3 rounded-xl space-y-2"
                        style={{
                          background: 'var(--bg-card)',
                          border: '1px solid var(--border-color)',
                        }}
                      >
                        <div className="flex items-start justify-between">
                          <span
                            className="text-xs font-medium truncate pr-2"
                            style={{ color: 'var(--text-primary)' }}
                            title={s.file_name}
                          >
                            {s.file_name}
                          </span>
                          <span
                            className="text-[10px] font-medium px-2 py-0.5 rounded-full flex-shrink-0"
                            style={{
                              background:
                                s.status === 'PROCESSED'
                                  ? 'rgba(16,185,129,0.10)'
                                  : 'rgba(107,138,127,0.10)',
                              color: s.status === 'PROCESSED' ? '#10b981' : '#6b8a7f',
                            }}
                          >
                            {s.status}
                          </span>
                        </div>
                        <p className="text-[10px]" style={{ color: '#6b8a7f' }}>
                          Uploaded {formatCustomDate(s.date)}
                        </p>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </aside>
  );
}
