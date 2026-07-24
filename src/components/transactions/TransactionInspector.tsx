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
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { getErrorToast } from '@/lib/errorMapping';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { formatMoney } from '@/lib/formatMoney';
import { useToast } from '@/hooks/use-toast';
import { useTransactionForm } from './useTransactionForm';
import { TransactionAmountBalance } from './TransactionAmountBalance';
import { CategorySelect } from './CategorySelect';
import { InfoRow } from '@/components/ui/InfoRow';
import { instrumentIcon, instrumentTypeLabel } from '@/components/instruments/instrumentTypes';
import SourceEvidencePanel from './SourceEvidencePanel';
import EmiInstallmentTimeline from './EmiInstallmentTimeline';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
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



function SectionCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="bg-[#F8E7C9]/50 rounded-xl overflow-hidden border border-[#064E3B]/10">
      {children}
    </div>
  );
}

/**
 * Right-side inspector panel that slides in when a transaction is selected.
 * Contains the full TransactionDetail functionality (edit merchant, category,
 * notes, tags; view source evidence; EMI timeline) without navigating away
 * from the table.
 */
export default function TransactionInspector({
  transactionId,
  onClose,
  categories,
}: TransactionInspectorProps) {
  const navigate = useNavigate();
  const { toast } = useToast();
  const [activeTab, setActiveTab] = useState<Tab>('details');

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
    newTag,
    setNewTag,
    showSavedConfirm,
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

  if (!transactionId) return null;

  const TABS: { id: Tab; label: string; disabled?: boolean }[] = [
    { id: 'details', label: 'Details' },
    { id: 'evidence', label: 'Evidence' },
    { id: 'emi', label: 'EMI', disabled: !hasEmi },
  ];

  return (
    <aside
      className="flex flex-col h-full w-full bg-[#F8E7C9]"
      role="complementary"
      aria-label="Transaction detail"
    >
      {/* Panel header */}
      <div
        className="flex items-start justify-between p-5 flex-shrink-0"
        style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
      >
        {isLoading || !tx ? (
          <div className="flex items-center gap-2" role="status" aria-label="Loading">
            <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/60" />
            <span className="text-[13px] text-[#064E3B]/60">Loading…</span>
          </div>
        ) : (
          <div className="min-w-0 flex-1 pr-3">
            {/* Merchant avatar + name */}
            <div className="flex items-center gap-3 mb-3">
              <div
                className="w-10 h-10 rounded-xl flex items-center justify-center text-[15px] font-bold flex-shrink-0"
                style={{ background: INK, color: CREAM }}
                aria-hidden="true"
              >
                {tx.merchant_display_name?.charAt(0).toUpperCase() ?? '?'}
              </div>
              <div className="min-w-0">
                <p className="text-[15px] font-semibold truncate text-[#064E3B]">
                  {tx.merchant_display_name}
                </p>
                <div className="flex items-center gap-1.5 flex-wrap">
                  {tx.best_event_time && (
                    <p className="text-[12px] text-[#064E3B]/60">
                      {formatCustomDate(tx.best_event_time)}
                    </p>
                  )}
                  {category && (
                    <span className="flex items-center gap-1 text-[11px] text-[#064E3B]/60">
                      <span
                        className="w-1.5 h-1.5 rounded-full"
                        style={{ background: category.color ?? '#064E3B' }}
                        aria-hidden="true"
                      />
                      {category.name}
                    </span>
                  )}
                </div>
              </div>
            </div>
            {/* Amount */}
            <div className="flex items-center gap-2 flex-wrap">
              <p
                className="text-3xl font-bold tracking-tight"
                style={{ color: isDebit ? '#dc2626' : '#059669' }}
              >
                {isDebit ? '−' : '+'}₹
                {Math.abs(amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
              </p>
              <span
                className="flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-semibold"
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
              </span>
              {tx.transaction_subtype && (
                <span
                  className="flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-semibold"
                  style={{ background: 'rgba(6,78,59,0.09)', color: INK }}
                >
                  <Repeat className="w-3 h-3" />
                  {tx.transaction_subtype}
                </span>
              )}
            </div>
            {isForeignCurrency && (
              <p className="text-[12px] text-[#064E3B]/60 mt-1">
                {formatMoney(tx.original_amount_minor, tx.original_currency)}
                {tx.exchange_rate ? ` · rate ${tx.exchange_rate.toFixed(4)}` : ''}
              </p>
            )}
          </div>
        )}

        <div className="flex items-center gap-1 flex-shrink-0">
          {/* Open full detail */}
          <button
            type="button"
            className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
            onClick={() => navigate(`/transactions/${transactionId}`)}
            aria-label="Open full page"
            title="Open full page"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          {/* Close */}
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
      <div
        className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1 overflow-x-auto"
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
              'px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap',
              activeTab === tab.id
                ? 'bg-[#064E3B] text-[#F8E7C9]'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10',
              tab.disabled && 'opacity-50 cursor-not-allowed hover:bg-transparent'
            )}
            onClick={() => !tab.disabled && setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-y-auto px-5 py-4">
        {isLoading || !tx ? (
          <div className="flex items-center justify-center py-12" role="status">
            <Loader2 className="w-5 h-5 animate-spin" style={{ color: '#064E3B' }} />
          </div>
        ) : (
          <>
            {/* ── Details tab ──────────────────────────────── */}
            {activeTab === 'details' && (
              <div id="panel-details" role="tabpanel" className="space-y-4">
                {/* Transaction info */}
                <SectionCard>
                  <InfoRow label="Status">
                    <span
                      className="px-2 py-0.5 text-[11px] font-medium rounded-full"
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
                  {instrument && (
                    <InfoRow
                      icon={instrumentIcon(instrument.instrument_type, 13)}
                      label="Instrument"
                    >
                      <span className="flex flex-col items-end">
                        <span>{instrument.issuer_name}</span>
                        <span className="text-[11px] text-[#064E3B]/50 font-normal">
                          {instrumentTypeLabel(instrument.instrument_type)}
                          {instrument.masked_identifier ? ` · ${instrument.masked_identifier}` : ''}
                        </span>
                      </span>
                    </InfoRow>
                  )}
                  {tx.reference_id && (
                    <InfoRow icon={<Hash className="w-3 h-3" />} label="Reference ID">
                      <span className="font-mono">{tx.reference_id}</span>
                    </InfoRow>
                  )}
                  {tx.best_posting_date && (
                    <InfoRow label="Posting Date">{tx.best_posting_date}</InfoRow>
                  )}
                  {tx.location && (
                    <InfoRow icon={<MapPin className="w-3 h-3" />} label="Location">
                      {tx.location}
                    </InfoRow>
                  )}
                  <InfoRow icon={<Hash className="w-3 h-3" />} label="Transaction ID">
                    <span className="font-mono">{tx.id}</span>
                  </InfoRow>
                </SectionCard>

                {/* Amount & balance */}
                {(tx.balance_after_transaction !== null || isForeignCurrency) && (
                  <SectionCard>
                    <TransactionAmountBalance tx={tx} isForeignCurrency={isForeignCurrency} />
                  </SectionCard>
                )}

                {/* Provenance / confidence */}
                <SectionCard>
                  {tx.source_mix && (
                    <InfoRow icon={<Link2 className="w-3 h-3" />} label="Source">
                      {tx.source_mix}
                    </InfoRow>
                  )}
                  {tx.match_confidence && (
                    <InfoRow icon={<ShieldCheck className="w-3 h-3" />} label="Match Confidence">
                      {tx.match_confidence}
                    </InfoRow>
                  )}
                  {tx.event_time_confidence && (
                    <InfoRow label="Time Confidence">{tx.event_time_confidence}</InfoRow>
                  )}
                  {tx.alert_fired !== null && (
                    <InfoRow label="Alert Sent">{tx.alert_fired ? 'Yes' : 'No'}</InfoRow>
                  )}
                </SectionCard>

                {/* Editable fields */}
                <div className="bg-[#F8E7C9]/50 rounded-xl overflow-hidden border border-[#064E3B]/10">
                  <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                    <Label
                      htmlFor="insp-merchant"
                      className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                    >
                      Merchant
                    </Label>
                    <Input
                      id="insp-merchant"
                      value={merchant}
                      onChange={(e) => setMerchant(e.target.value)}
                      className="h-7 text-[13px] border-none shadow-none p-0 bg-transparent focus-visible:ring-0 text-[#064E3B]"
                      onKeyDown={(e) => e.key === 'Enter' && handleSave()}
                    />
                  </div>

                  <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                    <Label
                      htmlFor="insp-category"
                      className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                    >
                      Category
                    </Label>
                    <CategorySelect
                      categoryId={categoryId}
                      onChange={setCategoryId}
                      categories={categories}
                      id="insp-category"
                      triggerClassName="h-7 text-[13px] border-none shadow-none p-0 bg-transparent focus:ring-0 text-[#064E3B]"
                    />
                  </div>

                  <div className="flex flex-col gap-1 p-3 border-b border-[#064E3B]/5">
                    <Label
                      htmlFor="insp-notes"
                      className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60"
                    >
                      Notes
                    </Label>
                    <Textarea
                      id="insp-notes"
                      value={notes}
                      onChange={(e) => setNotes(e.target.value)}
                      placeholder="Add a note…"
                      rows={2}
                      className="text-[13px] border-none shadow-none p-0 bg-transparent focus-visible:ring-0 text-[#064E3B] resize-none min-h-0"
                    />
                  </div>

                  {/* Tags */}
                  <div className="flex flex-col gap-2 p-3">
                    <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                      Tags
                    </p>
                    <div className="flex flex-wrap gap-1.5 min-h-[28px]">
                      {tags.map((tag) => (
                        <span
                          key={tag}
                          className="inline-flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full"
                          style={{ background: 'rgba(6,78,59,0.09)', color: '#064E3B' }}
                        >
                          {tag}
                          <button
                            type="button"
                            onClick={() => handleRemoveTag(tag)}
                            aria-label={`Remove tag ${tag}`}
                            className="rounded-full hover:bg-[#064E3B]/10"
                            style={{ color: 'rgba(6,78,59,0.50)' }}
                          >
                            <X className="w-2.5 h-2.5" />
                          </button>
                        </span>
                      ))}
                    </div>
                    <div className="flex gap-2">
                      <input
                        type="text"
                        aria-label="New tag"
                        placeholder="New tag…"
                        list="insp-tag-suggestions"
                        value={newTag}
                        onChange={(e) => setNewTag(e.target.value)}
                        onKeyDown={(e) => e.key === 'Enter' && handleAddTag()}
                        className="flex-1 h-8 px-2 rounded-md text-[13px] border outline-none bg-white/50 border-[#064E3B]/10 focus:border-[#064E3B]/30 text-[#064E3B]"
                      />
                      <TagDatalist
                        id="insp-tag-suggestions"
                        tags={tags}
                        availableTags={availableTags}
                      />
                      <button
                        type="button"
                        onClick={handleAddTag}
                        className="w-8 h-8 flex items-center justify-center rounded-md hover:bg-[#064E3B]/20 transition-colors"
                        style={{ background: 'rgba(6,78,59,0.09)', color: '#064E3B' }}
                        aria-label="Add tag"
                      >
                        <Plus className="w-4 h-4" />
                      </button>
                    </div>
                  </div>
                </div>

                {/* Save confirmation */}
                {showSavedConfirm && (
                  <p
                    role="status"
                    className="flex items-center gap-1.5 text-xs font-medium"
                    style={{ color: '#10b981' }}
                  >
                    <CheckCircle2 className="w-3.5 h-3.5" aria-hidden="true" />
                    Thanks, we'll remember this.
                  </p>
                )}

                {/* Actions */}
                <div className="flex flex-col gap-2 pt-2">
                  <button
                    type="button"
                    onClick={handleSave}
                    disabled={updateFields.isPending}
                    className="w-full h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-[#064E3B]"
                    style={{ background: '#064E3B', color: '#F8E7C9' }}
                  >
                    {updateFields.isPending ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 animate-spin" />
                        Saving…
                      </>
                    ) : (
                      <>
                        <Save className="w-3.5 h-3.5" aria-hidden="true" />
                        Save Changes
                      </>
                    )}
                  </button>
                  <button
                    type="button"
                    onClick={handleDelete}
                    disabled={softDelete.isPending}
                    className="w-full h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors hover:bg-red-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-red-500"
                    style={{ color: '#dc2626' }}
                  >
                    {softDelete.isPending ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 animate-spin" />
                        Deleting…
                      </>
                    ) : (
                      <>
                        <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
                        Delete Transaction
                      </>
                    )}
                  </button>
                </div>

                {(tx.created_at || tx.updated_at) && (
                  <p className="text-[10px] text-[#064E3B]/40 text-center pt-1">
                    {tx.created_at && `Recorded ${formatCustomDate(tx.created_at)}`}
                    {tx.updated_at &&
                      tx.updated_at !== tx.created_at &&
                      ` · Updated ${formatCustomDate(tx.updated_at)}`}
                  </p>
                )}
              </div>
            )}

            {/* ── Evidence tab ─────────────────────────────── */}
            {activeTab === 'evidence' && (
              <div id="panel-evidence" role="tabpanel">
                <SourceEvidencePanel
                  transactionId={transactionId!}
                  observations={detail?.observations ?? []}
                />
              </div>
            )}

            {/* ── EMI tab ─────────────────────────────────── */}
            {activeTab === 'emi' && hasEmi && (
              <div id="panel-emi" role="tabpanel">
                <EmiInstallmentTimeline emiGroupId={tx.emi_group_id!} />
              </div>
            )}
          </>
        )}
      </div>
    </aside>
  );
}
