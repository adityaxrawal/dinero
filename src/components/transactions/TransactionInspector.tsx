/**
 * Side panel for viewing and editing one transaction.
 *
 * Combines the editable fields with source evidence, so a correction can be made
 * against the original message rather than from memory.
 */
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
  ArrowLeftRight,
  Repeat,
  ShieldCheck,
  Building2,
  Tag as TagIcon,
  SlidersHorizontal,
  ChevronDown,
  ChevronUp,
  Pencil,
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { cn, channelLabel } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { formatMoney } from '@/lib/formatMoney';
import { useTransactionForm } from './useTransactionForm';
import { TransactionAmountBalance } from './TransactionAmountBalance';
import { CategorySelect } from './CategorySelect';
import { InstrumentSelect } from '@/components/instruments/InstrumentSelect';
import SourceEvidencePanel from './SourceEvidencePanel';
import EmiInstallmentTimeline from './EmiInstallmentTimeline';
import { TagDatalist } from '@/components/transactions/TagDatalist';
import {
  MerchantField,
  TagsHeader,
  EmptyTagsNotice,
  TransactionAuditRows,
} from '@/components/transactions/TransactionFields';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import type {
  CategoryRecord,
  TagRecord,
  TransactionObservation,
  CanonicalTransaction,
} from '@/lib/ipc';

type Tab = 'details' | 'evidence' | 'emi';

interface TransactionInspectorProps {
  transactionId: string | null;
  onClose: () => void;
  categories: CategoryRecord[];
}

const INK = '#064E3B';
const CREAM = '#F8E7C9';

/** Titled card grouping one section of the inspector. */
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

const DEBIT_COLOR = '#dc2626';
const CREDIT_COLOR = '#059669';

/** Small pill for a single piece of transaction metadata. */
function MetaChip({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#064E3B]/5 border border-[#064E3B]/10',
        className
      )}
    >
      {children}
    </span>
  );
}

/** One prominent figure in the inspector header. */
function HeroStat({
  show,
  tx,
  isDebit,
  amountStr,
  setAmountStr,
  setDirection,
  category,
  isForeignCurrency,
}: {
  show: boolean;
  tx: CanonicalTransaction | undefined;
  isDebit: boolean;
  amountStr: string;
  setAmountStr: (value: string) => void;
  setDirection: (value: 'debit' | 'credit') => void;
  category: { name: string; color: string | null } | null | undefined;
  isForeignCurrency: boolean;
}) {
  if (!show || !tx) return null;
  const accent = isDebit ? DEBIT_COLOR : CREDIT_COLOR;
  const isPosted = (tx.status ?? '').toLowerCase() === 'posted';

  return (
    <div className="px-5 pt-4 pb-3 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
      <div className="flex items-baseline justify-between gap-3 mb-2">
        <div className="flex items-center gap-1">
          <span className="text-3xl font-extrabold font-mono" style={{ color: accent }}>
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
              style={{ color: accent, width: `${Math.max(amountStr.length * 18 + 28, 90)}px` }}
            />
            <Pencil
              className="w-3.5 h-3.5 opacity-70 group-hover:opacity-100 transition-opacity absolute right-0 pointer-events-none"
              style={{ color: accent }}
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
              color: accent,
            }}
          >
            {isDebit ? <ArrowUpRight className="w-3 h-3" /> : <ArrowDownLeft className="w-3 h-3" />}
            {isDebit ? 'Debit' : 'Credit'}
          </button>

          {tx.status && (
            <span
              className="px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider rounded-full shadow-2xs"
              style={{
                background: isPosted ? 'rgba(16,185,129,0.15)' : 'rgba(107,138,127,0.15)',
                color: isPosted ? CREDIT_COLOR : '#064E3B',
              }}
            >
              {tx.status}
            </span>
          )}
        </div>
      </div>

      <MetaChipRow tx={tx} category={category} isForeignCurrency={isForeignCurrency} />
    </div>
  );
}

/**
 * Row of metadata chips: channel, reference, status.
 *
 * Each chip renders only when its value exists, so a sparsely extracted
 * transaction shows a short row rather than a line of empty placeholders.
 */
function MetaChipRow({
  tx,
  category,
  isForeignCurrency,
}: {
  tx: CanonicalTransaction;
  category: { name: string; color: string | null } | null | undefined;
  isForeignCurrency: boolean;
}) {
  return (
    <div className="flex items-center gap-2 flex-wrap text-[12px] text-[#064E3B]/70 font-medium">
      {category && (
        <MetaChip>
          <span
            className="w-2 h-2 rounded-full"
            style={{ background: category.color ?? '#064E3B' }}
            aria-hidden="true"
          />
          {category.name}
        </MetaChip>
      )}
      {tx.transaction_subtype && (
        <MetaChip>
          <Repeat className="w-3 h-3 text-[#064E3B]/60" />
          {tx.transaction_subtype}
        </MetaChip>
      )}
      {tx.channel && (
        <MetaChip>
          <ArrowLeftRight className="w-3 h-3 text-[#064E3B]/60" />
          {channelLabel(tx.channel)}
        </MetaChip>
      )}
      {isForeignCurrency && (
        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-amber-500/10 text-amber-800 border border-amber-500/20 font-mono text-[11px]">
          {formatMoney(tx.original_amount_minor, tx.original_currency)}
          {tx.exchange_rate ? ` @ ${tx.exchange_rate.toFixed(4)}` : ''}
        </span>
      )}
    </div>
  );
}

/** Tab bar switching between details, evidence and EMI panels. */
function InspectorTabs({
  tabs,
  activeTab,
  onSelect,
}: {
  tabs: { id: Tab; label: string; count?: number; disabled?: boolean }[];
  activeTab: Tab;
  onSelect: (id: Tab) => void;
}) {
  return (
    <div
      className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1.5 border-b border-[#064E3B]/10 overflow-x-auto bg-[#F8E7C9]/40"
      role="tablist"
      aria-label="Transaction panels"
    >
      {tabs.map((tab) => {
        const isActive = activeTab === tab.id;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            aria-controls={`panel-${tab.id}`}
            disabled={tab.disabled}
            className={cn(
              'flex items-center gap-1.5 px-3.5 py-1.5 text-[12px] font-semibold rounded-full transition-all whitespace-nowrap cursor-pointer',
              isActive
                ? 'bg-[#064E3B] text-[#F8E7C9] shadow-xs'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10 hover:text-[#064E3B]',
              tab.disabled && 'opacity-40 cursor-not-allowed hover:bg-transparent'
            )}
            onClick={() => !tab.disabled && onSelect(tab.id)}
          >
            {tab.label}
            {tab.count !== undefined && tab.count > 0 && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  isActive ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {tab.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Tag editing within the inspector.
 *
 * Suggests existing tags while typing, which is what stops near-duplicate tags
 * accumulating from small spelling differences.
 */
function InspectorTagEditor({
  tags,
  availableTags,
  newTag,
  setNewTag,
  handleAddTag,
  handleRemoveTag,
}: {
  tags: string[];
  availableTags: TagRecord[];
  newTag: string;
  setNewTag: (value: string) => void;
  handleAddTag: () => void;
  handleRemoveTag: (tag: string) => void;
}) {
  return (
  <div className="space-y-2 pt-2 border-t border-[#064E3B]/10">
    <TagsHeader count={tags.length} />
    <div className="flex flex-wrap gap-1.5 min-h-[30px] items-center">
      {tags.length === 0 ? (
        <EmptyTagsNotice />
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
  );
}

type TransactionForm = ReturnType<typeof useTransactionForm>;

/** The editable field set: merchant, category, amount, date, instrument, notes. */
function DetailsPanel({
  show,
  tx,
  form,
  categories,
  isAuditOpen,
  setIsAuditOpen,
}: {
  show: boolean;
  tx: CanonicalTransaction;
  form: TransactionForm;
  categories: CategoryRecord[];
  isAuditOpen: boolean;
  setIsAuditOpen: React.Dispatch<React.SetStateAction<boolean>>;
}) {
  if (!show) return null;
  return (
    <div id="panel-details" role="tabpanel" className="space-y-4 animate-in fade-in-50 duration-200">
      <SectionCard title="Metadata & Categorization" icon={<SlidersHorizontal className="w-3.5 h-3.5" />}>
        <div className="p-4 space-y-4">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <Label htmlFor="insp-form.merchant" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                  Merchant Name
                </Label>
                {form.merchant !== (tx.merchant_display_name ?? '') && (
                  <button
                    type="button"
                    onClick={() => form.setMerchant(tx.merchant_display_name ?? '')}
                    className="text-[10px] font-semibold text-[#064E3B]/60 hover:text-[#064E3B] underline cursor-pointer"
                  >
                    Reset
                  </button>
                )}
              </div>
              <MerchantField
                id="insp-merchant"
                merchant={form.merchant}
                onChange={form.setMerchant}
                onSubmit={form.handleSave}
              />
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="insp-category" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
                Category
              </Label>
              <CategorySelect
                categoryId={form.categoryId}
                onChange={form.setCategoryId}
                categories={categories}
                id="insp-category"
                triggerClassName="h-9 text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl w-full"
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="insp-notes" className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70">
              Notes
            </Label>
            <Textarea
              id="insp-notes"
              value={form.notes}
              onChange={(e) => form.setNotes(e.target.value)}
              placeholder="Add private notes or annotations…"
              rows={2}
              className="text-[13px] bg-[#F3EBDD]/70 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 focus-visible:border-[#064E3B]/40 rounded-xl resize-none min-h-[64px]"
            />
          </div>

          <InspectorTagEditor
            tags={form.tags}
            availableTags={form.availableTags}
            newTag={form.newTag}
            setNewTag={form.setNewTag}
            handleAddTag={form.handleAddTag}
            handleRemoveTag={form.handleRemoveTag}
          />
        </div>
      </SectionCard>

      <SectionCard title="Payment Instrument & Balance" icon={<Building2 className="w-3.5 h-3.5" />}>
        <InstrumentSelect
          instrumentId={form.instrumentId}
          onInstrumentChange={form.setInstrumentId}
          instruments={form.instruments}
        />

        {(tx.balance_after_transaction !== null || form.isForeignCurrency) && (
          <TransactionAmountBalance tx={tx} isForeignCurrency={form.isForeignCurrency} />
        )}
      </SectionCard>

      <SectionCard
        title="Audit & Technical Specs"
        icon={<ShieldCheck className="w-3.5 h-3.5" />}
        collapsible
        isExpanded={isAuditOpen}
        onToggle={() => setIsAuditOpen((prev) => !prev)}
      >
        <TransactionAuditRows tx={tx} />
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
  );
}

/**
 * Save and delete actions.
 *
 * Save is enabled only while the form is dirty, so an unchanged form cannot
 * issue a pointless write.
 */
function InspectorFooter({
  show,
  isDirty,
  showSavedConfirm,
  resetForm,
  handleSave,
  handleDelete,
  updateFields,
  softDelete,
}: {
  show: boolean;
  isDirty: boolean;
  showSavedConfirm: boolean;
  resetForm: () => void;
  handleSave: () => void;
  handleDelete: () => void;
  updateFields: { isPending: boolean };
  softDelete: { isPending: boolean };
}) {
  if (!show) return null;
  return (
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
  );
}

/** Header showing the transaction's headline amount, merchant and close control. */
function InspectorHeader({
  tx,
  onOpenFullPage,
  onClose,
}: {
  tx: CanonicalTransaction | undefined;
  onOpenFullPage: () => void;
  onClose: () => void;
}) {
  return (
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
          onClick={onOpenFullPage}
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
  );
}

/** Placeholder shown while the transaction detail query is in flight. */
function InspectorLoading() {
  return (
    <div className="flex flex-col items-center justify-center py-16 gap-3" role="status">
      <Loader2 className="w-6 h-6 animate-spin text-[#064E3B]" />
      <span className="text-[13px] font-medium text-[#064E3B]/60">Loading transaction…</span>
    </div>
  );
}

/**
 * Provenance panel: the source observations behind this transaction.
 *
 * Lets a user check a suspect value against what the bank actually sent, rather
 * than trusting the extracted figure.
 */
function EvidencePanel({
  show,
  transactionId,
  observations,
  currentBank,
}: {
  show: boolean;
  transactionId: string;
  observations: TransactionObservation[];
  currentBank: string | null;
}) {
  if (!show) return null;
  return (
    <div id="panel-evidence" role="tabpanel" className="animate-in fade-in-50 duration-200">
      <SourceEvidencePanel
        transactionId={transactionId}
        observations={observations}
        currentBank={currentBank}
      />
    </div>
  );
}

/** Instalment timeline, rendered only for transactions in an EMI group. */
function EmiPanel({ show, emiGroupId }: { show: boolean; emiGroupId: string | null }) {
  if (!show || !emiGroupId) return null;
  return (
    <div id="panel-emi" role="tabpanel" className="animate-in fade-in-50 duration-200">
      <EmiInstallmentTimeline emiGroupId={emiGroupId} />
    </div>
  );
}

/**
 * Side panel for viewing and editing one transaction.
 *
 * Composes header, tabs and footer around whichever panel is selected. Form
 * state and mutations live in dedicated hooks, so this file stays concerned with
 * layout and leaves persistence logic elsewhere.
 */
export default function TransactionInspector({
  transactionId,
  onClose,
  categories,
}: TransactionInspectorProps) {
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<Tab>('details');
  const [isAuditOpen, setIsAuditOpen] = useState(false);

  const form = useTransactionForm(transactionId ?? undefined, onClose);
  const {
    detail,
    isLoading,
    amountStr,
    setAmountStr,
    setDirection,
    showSavedConfirm,
    isDirty,
    resetForm,
    updateFields,
    softDelete,
    tx,
    hasEmi,
    isDebit,
    category,
    isForeignCurrency,
    handleSave,
    handleDelete,
  } = form;

  useEffect(() => {
    setActiveTab('details');
  }, [transactionId]);

  useSaveShortcut(isDirty, handleSave);

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
      <InspectorHeader
        tx={tx}
        onOpenFullPage={() => navigate(`/transactions/${transactionId}`)}
        onClose={onClose}
      />

      <HeroStat
        show={!isLoading && !!tx}
        tx={tx}
        isDebit={isDebit}
        amountStr={amountStr}
        setAmountStr={setAmountStr}
        setDirection={setDirection}
        category={category}
        isForeignCurrency={isForeignCurrency}
      />

      <InspectorTabs tabs={TABS} activeTab={activeTab} onSelect={setActiveTab} />

      <InspectorBody
        form={form}
        transactionId={transactionId}
        activeTab={activeTab}
        categories={categories}
        isAuditOpen={isAuditOpen}
        setIsAuditOpen={setIsAuditOpen}
      />

      <InspectorFooter
          show={!isLoading && !!tx}
          isDirty={isDirty}
          showSavedConfirm={showSavedConfirm}
          resetForm={resetForm}
          handleSave={handleSave}
          handleDelete={handleDelete}
          updateFields={updateFields}
          softDelete={softDelete}
        />
    </aside>
  );
}

/**
 * Binds Cmd/Ctrl+S to save while the form has unsaved changes.
 *
 * Gated on dirtiness so the shortcut cannot fire a redundant write, and so the
 * browser's own save dialog is only suppressed when there is genuinely something
 * to save.
 */
function useSaveShortcut(isDirty: boolean, handleSave: () => void) {
  useEffect(() => {
    /** Intercepts Cmd/Ctrl+S so the browser's save dialog does not open. */
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        if (isDirty) handleSave();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isDirty, handleSave]);
}

/** Routes the selected tab to its panel. */
function InspectorBody({
  form,
  transactionId,
  activeTab,
  categories,
  isAuditOpen,
  setIsAuditOpen,
}: {
  form: ReturnType<typeof useTransactionForm>;
  transactionId: string;
  activeTab: Tab;
  categories: CategoryRecord[];
  isAuditOpen: boolean;
  setIsAuditOpen: React.Dispatch<React.SetStateAction<boolean>>;
}) {
  const { detail, isLoading, tx, hasEmi, instrument } = form;

  return (
    <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
      {isLoading || !tx ? (
        <InspectorLoading />
      ) : (
        <>
          <DetailsPanel
            show={activeTab === 'details'}
            tx={tx}
            form={form}
            categories={categories}
            isAuditOpen={isAuditOpen}
            setIsAuditOpen={setIsAuditOpen}
          />

          <EvidencePanel
            show={activeTab === 'evidence'}
            transactionId={transactionId}
            observations={detail?.observations ?? []}
            currentBank={instrument?.issuer_name ?? null}
          />

          <EmiPanel show={activeTab === 'emi' && hasEmi} emiGroupId={tx.emi_group_id} />
        </>
      )}
    </div>
  );
}
