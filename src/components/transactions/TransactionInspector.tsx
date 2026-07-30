import { useState, useEffect } from 'react';
import {
  X,
  Loader2,
  Save,
  Trash2,
  Plus,
  ExternalLink,
  CheckCircle2,
  ArrowDownLeft,
  ArrowUpRight,
  Repeat,
  Landmark,
  MapPin,
  Hash,
  ShieldCheck,
  Link2,
  Building2,
  Tag as TagIcon,
  FileText,
  Clock,
  SlidersHorizontal,
  ChevronDown,
  ChevronUp,
  Pencil,
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { formatMoney } from '@/lib/formatMoney';
import { useTransactionForm } from './useTransactionForm';
import { TransactionAmountBalance } from './TransactionAmountBalance';
import { CategorySelect } from './CategorySelect';
import { InfoRow } from '@/components/ui/InfoRow';
import { instrumentIcon, instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import { InstrumentSelect } from '@/components/instruments/InstrumentSelect';
import SourceEvidencePanel from './SourceEvidencePanel';
import EmiInstallmentTimeline from './EmiInstallmentTimeline';
import { TagDatalist } from '@/components/transactions/TagDatalist';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import type { CategoryRecord } from '@/lib/ipc';

type Tab = 'details' | 'evidence' | 'emi';

interface TransactionInspectorProps {
  transactionId: string | null;
  onClose: () => void;
  categories: CategoryRecord[];
}

const INK = '#064E3B';
const CREAM = '#F8E7C9';

function SectionCard({
  title,
  icon,
  children,
  className,
  collapsible,
  isExpanded = true,
  onToggle,
  badge,
}: {
  title?: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  collapsible?: boolean;
  isExpanded?: boolean;
  onToggle?: () => void;
  badge?: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        'bg-[#F8E7C9]/60 backdrop-blur-sm rounded-2xl overflow-hidden border border-[#064E3B]/10 shadow-xs transition-all hover:border-[#064E3B]/20',
        className
      )}
    >
      {title && (
        <div
          className={cn(
            'flex items-center justify-between px-4 py-3 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]',
            collapsible && 'cursor-pointer select-none hover:bg-[#064E3B]/[0.06] transition-colors'
          )}
          onClick={collapsible ? onToggle : undefined}
        >
          <div className="flex items-center gap-2 min-w-0">
            {icon && <span className="text-[#064E3B]/70 shrink-0">{icon}</span>}
            <h3 className="text-[12px] font-semibold tracking-wider text-[#064E3B] uppercase truncate">
              {title}
            </h3>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {badge}
            {collapsible && (
              <span className="text-[#064E3B]/60">
                {isExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              </span>
            )}
          </div>
        </div>
      )}
      {(!collapsible || isExpanded) && children}
    </div>
  );
}

/**
 * Right-side inspector panel that slides in when a transaction is selected.
 * Contains full transaction detail viewing, inline field editing (merchant, category,
 * notes, tags), source evidence analysis, and EMI timeline.
 */
export default function TransactionInspector({
  transactionId,
  onClose,
  categories,
}: TransactionInspectorProps) {
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<Tab>('details');
  const [isAuditOpen, setIsAuditOpen] = useState(false);

  const {
    detail,
    isLoading,
    tags,
    availableTags,
    merchant,
    setMerchant,
    categoryId,
    setCategoryId,
    notes,
    setNotes,
    amountStr,
    setAmountStr,
    direction,
    setDirection,
    eventTime,
    setEventTime,
    instrumentId,
    setInstrumentId,
    instruments,
    newTag,
    setNewTag,
    showSavedConfirm,
    isDirty,
    resetForm,
    updateFields,
    softDelete,
    tx,
    amount,
    hasEmi,
    isDebit,
    instrument,
    category,
    isForeignCurrency,
    handleSave,
    handleAddTag,
    handleRemoveTag,
    handleDelete,
  } = useTransactionForm(transactionId ?? undefined, onClose);

  // Reset tab when new transaction selected
  useEffect(() => {
    setActiveTab('details');
  }, [transactionId]);

  // Keyboard shortcut: Cmd/Ctrl + S to save changes
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        if (isDirty) {
          handleSave();
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isDirty, handleSave]);

  if (!transactionId) return null;

  const observationCount = detail?.observations?.length ?? 0;

  const TABS: { id: Tab; label: string; count?: number; disabled?: boolean }[] = [
    { id: 'details', label: 'Details' },
    { id: 'evidence', label: 'Evidence', count: observationCount },
    { id: 'emi', label: 'EMI', disabled: !hasEmi },
  ];

  return (
    <aside
      className="flex flex-col h-full w-full bg-[#F8E7C9] selection:bg-[#064E3B] selection:text-[#F8E7C9]"
      role="complementary"
      aria-label="Transaction detail"
    >
      {/* ── Header Bar ───────────────────────────────────────── */}
      <div
        className="flex items-center justify-between px-5 py-4 flex-shrink-0 bg-[#F8E7C9]/80 border-b border-[#064E3B]/10 backdrop-blur-md"
      >
        <div className="flex items-center gap-3 min-w-0">
          <div
            className="w-10 h-10 rounded-xl flex items-center justify-center text-[16px] font-bold shadow-xs shrink-0 ring-2 ring-[#064E3B]/10"
            style={{ background: INK, color: CREAM }}
            aria-hidden="true"
          >
            {tx?.merchant_display_name?.charAt(0).toUpperCase() ?? '?'}
          </div>
          <div className="min-w-0">
            <h2 className="text-[15px] font-bold truncate text-[#064E3B]">
              {tx?.merchant_display_name || 'Transaction Details'}
            </h2>
            {tx?.best_event_time && (
              <p className="text-[12px] font-medium text-[#064E3B]/60 truncate">
                {formatCustomDate(tx.best_event_time)}
              </p>
            )}
          </div>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B]"
            onClick={() => navigate(`/transactions/${transactionId}`)}
            aria-label="Open full page"
            title="Open full page view"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B]"
            onClick={onClose}
            aria-label="Close inspector"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
      </div>

      {/* ── Sub Header / Hero Stat Card ────────────────────── */}
      {!isLoading && tx && (
        <div className="px-5 pt-4 pb-3 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
          <div className="flex items-baseline justify-between gap-3 mb-2">
            <div className="flex items-center gap-1">
              <span className="text-3xl font-extrabold font-mono" style={{ color: isDebit ? '#dc2626' : '#059669' }}>
                {isDebit ? '−' : '+'}₹
              </span>
              <div className="relative flex items-center group">
                <input
                  type="number"
                  step="0.01"
                  value={amountStr}
                  onChange={(e) => setAmountStr(e.target.value)}
                  aria-label="Transaction Amount"
                  className="bg-transparent outline-none border-b-2 border-dashed border-current focus:border-solid text-3xl font-extrabold font-mono [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none pr-6"
                  style={{
                    color: isDebit ? '#dc2626' : '#059669',
                    width: `${Math.max(amountStr.length * 18 + 28, 90)}px`,
                  }}
                />
                <Pencil
                  className="w-3.5 h-3.5 opacity-70 group-hover:opacity-100 transition-opacity absolute right-0 pointer-events-none"
                  style={{ color: isDebit ? '#dc2626' : '#059669' }}
                />
              </div>
            </div>

            <div className="flex items-center gap-1.5 flex-wrap">
              <button
                type="button"
                onClick={() => setDirection(isDebit ? 'credit' : 'debit')}
                className="flex items-center gap-1 px-2.5 py-1 rounded-full text-[11px] font-semibold tracking-wide uppercase shadow-2xs cursor-pointer hover:opacity-80 transition-opacity"
                title="Click to toggle Debit / Credit"
                style={{
                  background: isDebit ? 'rgba(220,38,38,0.12)' : 'rgba(5,150,105,0.12)',
                  color: isDebit ? '#dc2626' : '#059669',
                }}
              >
                {isDebit ? (
                  <ArrowUpRight className="w-3 h-3" />
                ) : (
                  <ArrowDownLeft className="w-3 h-3" />
                )}
                {isDebit ? 'Debit' : 'Credit'}
              </button>

              {tx.status && (
                <span
                  className="px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider rounded-full shadow-2xs"
                  style={{
                    background:
                      (tx.status ?? '').toLowerCase() === 'posted'
                        ? 'rgba(16,185,129,0.15)'
                        : 'rgba(107,138,127,0.15)',
                    color: (tx.status ?? '').toLowerCase() === 'posted' ? '#059669' : '#064E3B',
                  }}
                >
                  {tx.status}
                </span>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2 flex-wrap text-[12px] text-[#064E3B]/70 font-medium">
            {category && (
              <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#064E3B]/5 border border-[#064E3B]/10">
                <span
                  className="w-2 h-2 rounded-full"
                  style={{ background: category.color ?? '#064E3B' }}
                  aria-hidden="true"
                />
                {category.name}
              </span>
            )}
            {tx.transaction_subtype && (
              <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-[#064E3B]/5 border border-[#064E3B]/10">
                <Repeat className="w-3 h-3 text-[#064E3B]/60" />
                {tx.transaction_subtype}
              </span>
            )}
            {isForeignCurrency && (
              <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-amber-500/10 text-amber-800 border border-amber-500/20 font-mono text-[11px]">
                {formatMoney(tx.original_amount_minor, tx.original_currency)}
                {tx.exchange_rate ? ` @ ${tx.exchange_rate.toFixed(4)}` : ''}
              </span>
            )}
          </div>
        </div>
      )}

      {/* ── Tabs Navigation ──────────────────────────────────── */}
      <div
        className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1.5 border-b border-[#064E3B]/10 overflow-x-auto bg-[#F8E7C9]/40"
        role="tablist"
        aria-label="Transaction panels"
      >
        {TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            aria-controls={`panel-${tab.id}`}
            disabled={tab.disabled}
            className={cn(
              'flex items-center gap-1.5 px-3.5 py-1.5 text-[12px] font-semibold rounded-full transition-all whitespace-nowrap cursor-pointer',
              activeTab === tab.id
                ? 'bg-[#064E3B] text-[#F8E7C9] shadow-xs'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10 hover:text-[#064E3B]',
              tab.disabled && 'opacity-40 cursor-not-allowed hover:bg-transparent'
            )}
            onClick={() => !tab.disabled && setActiveTab(tab.id)}
          >
            {tab.label}
            {tab.count !== undefined && tab.count > 0 && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  activeTab === tab.id
                    ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
                    : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {tab.count}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* ── Main Tab Content Scroll Area ────────────────────── */}
      <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
        {isLoading || !tx ? (
          <div className="flex flex-col items-center justify-center py-16 gap-3" role="status">
            <Loader2 className="w-6 h-6 animate-spin text-[#064E3B]" />
            <span className="text-[13px] font-medium text-[#064E3B]/60">Loading transaction…</span>
          </div>
        ) : (
          <>
            {/* ── Details Tab ───────────────────────────────── */}
            {activeTab === 'details' && (
              <div id="panel-details" role="tabpanel" className="space-y-4 animate-in fade-in-50 duration-200">
                {/* 1. Transaction Metadata & Editing */}
                <SectionCard title="Metadata & Categorization" icon={<SlidersHorizontal className="w-3.5 h-3.5" />}>
                  <div className="p-4 space-y-4">
                    {/* 2-Column Grid for Merchant Name & Category */}
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                      {/* Merchant Name */}
                      <div className="space-y-1.5">
                        <div className="flex items-center justify-between">
                          <Label htmlFor="insp-merchant" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                            Merchant Name
                          </Label>
                          {merchant !== (tx.merchant_display_name ?? '') && (
                            <button
                              type="button"
                              onClick={() => setMerchant(tx.merchant_display_name ?? '')}
                              className="text-[10px] font-semibold text-[#064E3B]/60 hover:text-[#064E3B] underline cursor-pointer"
                            >
                              Reset
                            </button>
                          )}
                        </div>
                        <div className="relative">
                          <Input
                            id="insp-merchant"
                            value={merchant}
                            onChange={(e) => setMerchant(e.target.value)}
                            placeholder="Merchant name…"
                            className="h-9 text-[13px] font-semibold bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl pr-8"
                            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                          />
                          {merchant && (
                            <button
                              type="button"
                              onClick={() => setMerchant('')}
                              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/40 hover:text-[#064E3B]"
                              aria-label="Clear merchant name"
                            >
                              <X className="w-3.5 h-3.5" />
                            </button>
                          )}
                        </div>
                      </div>

                      {/* Category Selection */}
                      <div className="space-y-1.5">
                        <Label htmlFor="insp-category" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Category
                        </Label>
                        <CategorySelect
                          categoryId={categoryId}
                          onChange={setCategoryId}
                          categories={categories}
                          id="insp-category"
                          triggerClassName="h-9 text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl w-full"
                        />
                      </div>
                    </div>

                    {/* Notes */}
                    <div className="space-y-1.5">
                      <Label htmlFor="insp-notes" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                        Notes
                      </Label>
                      <Textarea
                        id="insp-notes"
                        value={notes}
                        onChange={(e) => setNotes(e.target.value)}
                        placeholder="Add private notes or annotations…"
                        rows={2}
                        className="text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl resize-none min-h-[64px]"
                      />
                    </div>

                    {/* Tags */}
                    <div className="space-y-2 pt-2 border-t border-[#064E3B]/10">
                      <div className="flex items-center justify-between">
                        <Label className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                          Tags
                        </Label>
                        <span className="text-[10px] text-[#064E3B]/50 font-mono">
                          {tags.length} tag{tags.length !== 1 ? 's' : ''}
                        </span>
                      </div>
                      <div className="flex flex-wrap gap-1.5 min-h-[30px] items-center">
                        {tags.length === 0 ? (
                          <span className="text-[12px] italic text-[#064E3B]/40">No tags added yet.</span>
                        ) : (
                          tags.map((tag) => (
                            <span
                              key={tag}
                              className="inline-flex items-center gap-1.5 text-[11px] font-semibold px-2.5 py-0.5 rounded-full border border-[#064E3B]/15 shadow-2xs transition-transform hover:scale-105"
                              style={{ background: 'rgba(6,78,59,0.08)', color: '#064E3B' }}
                            >
                              <TagIcon className="w-2.5 h-2.5 opacity-60" />
                              {tag}
                              <button
                                type="button"
                                onClick={() => handleRemoveTag(tag)}
                                aria-label={`Remove tag ${tag}`}
                                className="rounded-full p-0.5 hover:bg-[#064E3B]/20 text-[#064E3B]/60 hover:text-[#064E3B] cursor-pointer"
                              >
                                <X className="w-2.5 h-2.5" />
                              </button>
                            </span>
                          ))
                        )}
                      </div>

                      <div className="flex gap-2 pt-1">
                        <input
                          type="text"
                          aria-label="New tag"
                          placeholder="Add new tag..."
                          list="insp-tag-suggestions"
                          value={newTag}
                          onChange={(e) => setNewTag(e.target.value)}
                          onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), handleAddTag())}
                          className="flex-1 h-8 px-3 rounded-lg text-[12px] outline-none bg-[#F3EBDD]/70 border border-[#064E3B]/15 focus:border-[#064E3B]/40 text-[#064E3B] placeholder:text-[#064E3B]/40"
                        />
                        <TagDatalist
                          id="insp-tag-suggestions"
                          tags={tags}
                          availableTags={availableTags}
                        />
                        <button
                          type="button"
                          onClick={handleAddTag}
                          className="h-8 px-3 flex items-center justify-center gap-1 rounded-lg text-[12px] font-semibold transition-colors bg-[#064E3B]/10 hover:bg-[#064E3B]/20 text-[#064E3B] cursor-pointer"
                          aria-label="Add tag"
                        >
                          <Plus className="w-3.5 h-3.5" />
                          Add
                        </button>
                      </div>
                    </div>
                  </div>
                </SectionCard>

                {/* 2. Payment Instrument & Balance */}
                <SectionCard title="Payment Instrument & Balance" icon={<Building2 className="w-3.5 h-3.5" />}>
                  <InstrumentSelect
                    instrumentId={instrumentId}
                    onInstrumentChange={setInstrumentId}
                    instruments={instruments}
                  />

                  {(tx.balance_after_transaction !== null || isForeignCurrency) && (
                    <TransactionAmountBalance tx={tx} isForeignCurrency={isForeignCurrency} />
                  )}
                </SectionCard>

                {/* 3. Audit & Technical Specifications */}
                <SectionCard
                  title="Audit & Technical Specs"
                  icon={<ShieldCheck className="w-3.5 h-3.5" />}
                  collapsible
                  isExpanded={isAuditOpen}
                  onToggle={() => setIsAuditOpen((prev) => !prev)}
                >
                  <InfoRow label="Status">
                    <span
                      className="px-2 py-0.5 text-[11px] font-bold rounded-full uppercase tracking-wider"
                      style={{
                        background:
                          (tx.status ?? '').toLowerCase() === 'posted'
                            ? 'rgba(16,185,129,0.15)'
                            : 'rgba(107,138,127,0.15)',
                        color: (tx.status ?? '').toLowerCase() === 'posted' ? '#059669' : '#064E3B',
                      }}
                    >
                      {tx.status ?? 'UNKNOWN'}
                    </span>
                  </InfoRow>

                  {tx.best_posting_date && (
                    <InfoRow icon={<Clock className="w-3.5 h-3.5" />} label="Posting Date">
                      {tx.best_posting_date}
                    </InfoRow>
                  )}

                  {tx.reference_id && (
                    <InfoRow
                      icon={<Hash className="w-3.5 h-3.5" />}
                      label="Reference ID"
                      copyValue={tx.reference_id}
                    >
                      <span className="font-mono text-[12px]">{tx.reference_id}</span>
                    </InfoRow>
                  )}

                  <InfoRow
                    icon={<Hash className="w-3.5 h-3.5" />}
                    label="Transaction ID"
                    copyValue={tx.id}
                  >
                    <span className="font-mono text-[11px] opacity-90">{tx.id}</span>
                  </InfoRow>

                  {tx.location && (
                    <InfoRow icon={<MapPin className="w-3.5 h-3.5" />} label="Location">
                      {tx.location}
                    </InfoRow>
                  )}

                  {tx.source_mix && (
                    <InfoRow icon={<Link2 className="w-3.5 h-3.5" />} label="Source Pipeline">
                      <span className="font-mono text-[11px] uppercase bg-[#064E3B]/5 px-2 py-0.5 rounded border border-[#064E3B]/10">
                        {tx.source_mix}
                      </span>
                    </InfoRow>
                  )}

                  {tx.match_confidence && (
                    <InfoRow icon={<ShieldCheck className="w-3.5 h-3.5" />} label="Match Confidence">
                      <span className="capitalize">{tx.match_confidence}</span>
                    </InfoRow>
                  )}

                  {tx.event_time_confidence && (
                    <InfoRow label="Time Confidence">
                      <span className="capitalize">{tx.event_time_confidence}</span>
                    </InfoRow>
                  )}

                  {tx.alert_fired !== null && (
                    <InfoRow label="Alert Sent">
                      {tx.alert_fired ? 'Yes' : 'No'}
                    </InfoRow>
                  )}
                </SectionCard>

                {(tx.created_at || tx.updated_at) && (
                  <p className="text-[10px] text-[#064E3B]/50 text-center pt-1 font-mono">
                    {tx.created_at && `Recorded ${formatCustomDate(tx.created_at)}`}
                    {tx.updated_at &&
                      tx.updated_at !== tx.created_at &&
                      ` · Updated ${formatCustomDate(tx.updated_at)}`}
                  </p>
                )}
              </div>
            )}

            {/* ── Evidence Tab ──────────────────────────────── */}
            {activeTab === 'evidence' && (
              <div id="panel-evidence" role="tabpanel" className="animate-in fade-in-50 duration-200">
                <SourceEvidencePanel
                  transactionId={transactionId!}
                  observations={detail?.observations ?? []}
                  currentBank={instrument?.issuer_name ?? null}
                />
              </div>
            )}

            {/* ── EMI Tab ───────────────────────────────────── */}
            {activeTab === 'emi' && hasEmi && (
              <div id="panel-emi" role="tabpanel" className="animate-in fade-in-50 duration-200">
                <EmiInstallmentTimeline emiGroupId={tx.emi_group_id!} />
              </div>
            )}
          </>
        )}
      </div>

      {/* ── Sticky Action Footer ─────────────────────────────── */}
      {!isLoading && tx && (
        <div className="p-4 bg-[#F8E7C9]/90 border-t border-[#064E3B]/10 backdrop-blur-md flex flex-col gap-2 shrink-0">
          {showSavedConfirm && (
            <div
              role="status"
              className="flex items-center justify-center gap-1.5 text-xs font-semibold text-emerald-700 bg-emerald-500/10 py-1.5 px-3 rounded-lg border border-emerald-500/20 animate-in fade-in slide-in-from-bottom-1"
            >
              <CheckCircle2 className="w-4 h-4 text-emerald-600" aria-hidden="true" />
              Changes saved successfully.
            </div>
          )}

          {isDirty && !showSavedConfirm && (
            <div className="flex items-center justify-between text-[11px] font-semibold text-amber-800 bg-amber-500/10 py-1 px-3 rounded-lg border border-amber-500/20 animate-in fade-in">
              <span>Unsaved edits</span>
              <kbd className="font-mono text-[10px] bg-amber-500/10 px-1.5 py-0.5 rounded">⌘S to save</kbd>
            </div>
          )}

          <div className="flex gap-2">
            {isDirty && (
              <button
                type="button"
                onClick={resetForm}
                className="h-9 px-3 rounded-xl text-[12px] font-semibold transition-colors border border-[#064E3B]/20 bg-[#064E3B]/5 hover:bg-[#064E3B]/10 text-[#064E3B] cursor-pointer"
                title="Discard unsaved changes"
              >
                Reset
              </button>
            )}

            <button
              type="button"
              onClick={handleSave}
              disabled={updateFields.isPending || (!isDirty && !showSavedConfirm)}
              className={cn(
                'flex-1 h-9 rounded-xl text-[13px] font-bold flex items-center justify-center gap-2 transition-all shadow-xs cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-[#064E3B]',
                isDirty
                  ? 'bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 shadow-md ring-2 ring-[#064E3B]/30'
                  : 'bg-[#064E3B]/40 text-[#F8E7C9]/70 cursor-not-allowed'
              )}
            >
              {updateFields.isPending ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Saving Edits…
                </>
              ) : (
                <>
                  <Save className="w-4 h-4" aria-hidden="true" />
                  Save Changes
                </>
              )}
            </button>

            <button
              type="button"
              onClick={handleDelete}
              disabled={softDelete.isPending}
              className="h-9 px-3 rounded-xl text-[13px] font-bold flex items-center justify-center gap-1.5 transition-colors border border-red-500/20 bg-red-500/10 hover:bg-red-500/20 text-red-700 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-red-500"
              title="Delete Transaction"
            >
              {softDelete.isPending ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Trash2 className="w-4 h-4" aria-hidden="true" />
              )}
            </button>
          </div>
        </div>
      )}
    </aside>
  );
}

