import { useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { ShieldAlert, HelpCircle, Loader2, CheckCircle2, Layers } from 'lucide-react';

import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useUnassignedTransactions } from '@/hooks/queries/useUnassignedTransactions';
import ReconciliationInspector from '@/components/reconciliation/ReconciliationInspector';
import UnassignedInspector from '@/components/reconciliation/UnassignedInspector';
import { cn, formatRelativeDate, isOlderThanDays } from '@/lib/utils';
import { cleanTextForReader } from '@/components/common/gmailParsing';
import type { ClusterRecord, UnassignedTransactionRecord } from '@/lib/ipc';

// Body-text fingerprint -> display name, checked in order.
const BANK_FINGERPRINTS: [pattern: RegExp, name: string][] = [
  [/HDFC/i, 'HDFC Bank'],
  [/IndusInd/i, 'IndusInd Bank'],
  [/ICICI/i, 'ICICI Bank'],
  [/Axis/i, 'Axis Bank'],
  [/SBI/i, 'SBI Bank'],
  [/Kotak/i, 'Kotak Bank'],
];

const ISSUE_LABELS: Record<string, string> = {
  extraction_failed: 'Missing Fields',
  issuer_name_not_found: 'Unknown Card/Bank',
};

/** The Gmail payload is an opaque JSON blob; pull out the fields we display. */
function readAlertPayload(rawJson: unknown, fallbackText: string) {
  const empty = { name: '', subject: '', html: '', text: fallbackText };
  if (!rawJson || typeof rawJson !== 'string') return empty;
  try {
    const parsed = JSON.parse(rawJson);
    return {
      name: parsed.sender || parsed.from || '',
      subject: parsed.subject || '',
      html: parsed.html || '',
      text: parsed.body || fallbackText,
    };
  } catch {
    return empty;
  }
}

/** "IndusInd Bank <alerts@indusind.com>" -> "IndusInd Bank" */
function stripAddressFromName(name: string): string {
  if (!name.includes('<')) return name;
  return name.split('<')[0].replace(/^["']|["']$/g, '').trim();
}

/** Falls back to fingerprinting the body when the sender name says nothing. */
function resolveBankName(name: string, body: string): string {
  const isGeneric = !name || name === 'Bank Alert' || name === 'Bank / Service Alert';
  if (!isGeneric) return name;
  return BANK_FINGERPRINTS.find(([pattern]) => pattern.test(body))?.[1] || 'Bank Alert';
}

// Note: en-IN grouping (₹1,00,000), which lib/formatMoney.ts does not apply.
const formatRupees = (amountMinor: number | null | undefined): string | null =>
  amountMinor == null
    ? null
    : `₹${(amountMinor / 100).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

function getUnassignedDisplayInfo(item: UnassignedTransactionRecord) {
  const payload = readAlertPayload(item.raw_payload_json, item.body_snippet || '');
  const senderName = stripAddressFromName(payload.name || item.merchant_raw || '');

  const bodyText = cleanTextForReader(payload.html, payload.text);
  const name = resolveBankName(senderName, bodyText);

  // Snippets often repeat the bank name they open with — drop the echo.
  const trimmedBody = bodyText.startsWith(name) ? bodyText.slice(name.length).trim() : bodyText;

  return {
    name,
    snippet: payload.subject || trimmedBody || 'Transaction alert details missing',
    amountStr: formatRupees(item.amount_minor),
    dateStr: item.event_time
      ? new Date(item.event_time).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      : '',
    issueLabel: ISSUE_LABELS[item.reason] || 'Action Needed',
    avatarLetter: name.charAt(0).toUpperCase(),
  };
}

const ROW_CLASS =
  'flex flex-col w-full text-left px-3.5 py-3 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border';
const ROW_SELECTED = 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-sm';
const ROW_IDLE = 'bg-white/60 hover:bg-white text-[#064E3B] border-[#064E3B]/10 hover:border-[#064E3B]/20';

// Selected rows invert to the dark treatment, so every element swaps palette at
// once. Keeping them in one lookup avoids repeating the condition per element.
const CLUSTER_TONES = {
  selected: {
    row: ROW_SELECTED,
    badge: 'bg-[#F8E7C9]/20 text-[#F8E7C9]',
    meta: 'text-[#F8E7C9]/70',
    age: '',
    merchant: 'text-white',
  },
  idle: {
    row: ROW_IDLE,
    badge: 'bg-amber-100 text-amber-800 border border-amber-200',
    meta: 'text-slate-400',
    age: 'text-red-500 font-semibold',
    merchant: 'text-[#064E3B]',
  },
};

function ClusterListItem({
  cluster,
  isSelected,
  onSelect,
}: {
  cluster: ClusterRecord;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const tone = CLUSTER_TONES[isSelected ? 'selected' : 'idle'];
  const incoming = cluster.members.find((m) => m.member_role === 'incoming');
  const isDebit = incoming?.direction === 'debit';
  const amountStr = incoming ? `${isDebit ? '-' : '+'} ₹${Math.abs(incoming.amount).toFixed(2)}` : null;
  const ageStr = cluster.created_at ? formatRelativeDate(cluster.created_at) : null;
  const isStale = cluster.created_at ? isOlderThanDays(cluster.created_at, 3) : false;
  const amountTone = isSelected ? 'text-[#F8E7C9]' : isDebit ? 'text-red-700' : 'text-emerald-700';

  return (
    <button onClick={onSelect} className={cn(ROW_CLASS, tone.row)}>
      <div className="flex items-center justify-between gap-2 mb-1.5 w-full">
        <span className={cn('text-[9px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider', tone.badge)}>
          Ambiguous
        </span>
        <span className={cn('flex items-center gap-1.5 text-[10px] font-medium shrink-0', tone.meta)}>
          {ageStr && (
            <span className={cn(isStale && tone.age)} title={isStale ? 'Pending more than 3 days' : undefined}>
              {ageStr}
            </span>
          )}
          <span>{cluster.members_count} entries</span>
        </span>
      </div>
      <p className={cn('text-[13px] font-bold truncate mb-0.5', tone.merchant)}>
        {incoming?.merchant || 'Match requires review'}
      </p>
      {amountStr && <p className={cn('text-[12px] font-mono font-semibold', amountTone)}>{amountStr}</p>}
    </button>
  );
}

function UnassignedListItem({
  item,
  isSelected,
  onSelect,
}: {
  item: UnassignedTransactionRecord;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const info = getUnassignedDisplayInfo(item);

  return (
    <button onClick={onSelect} className={cn(ROW_CLASS, isSelected ? ROW_SELECTED : ROW_IDLE)}>
      <div className="flex items-center justify-between gap-2 mb-1.5 w-full">
        <div className="flex items-center gap-2 min-w-0">
          <div
            className={cn(
              'w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-bold shrink-0',
              isSelected ? 'bg-[#F8E7C9] text-[#064E3B]' : 'bg-[#064E3B]/10 text-[#064E3B]'
            )}
          >
            {info.avatarLetter}
          </div>
          <span
            className={cn('text-[13px] font-bold truncate tracking-tight', isSelected ? 'text-white' : 'text-slate-900')}
          >
            {info.name}
          </span>
        </div>

        <span
          className={cn(
            'text-[9px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider shrink-0',
            isSelected
              ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
              : item.reason === 'issuer_name_not_found'
                ? 'bg-purple-100 text-purple-700 border border-purple-200'
                : 'bg-amber-100 text-amber-800 border border-amber-200'
          )}
        >
          {info.issueLabel}
        </span>
      </div>

      <p className={cn('text-[11px] line-clamp-1 mb-2 font-normal', isSelected ? 'text-[#F8E7C9]/80' : 'text-slate-600')}>
        {info.snippet}
      </p>

      <div className="flex items-center justify-between text-[11px] font-medium pt-1 border-t border-current/10 w-full">
        {info.amountStr ? (
          <span className={cn('font-semibold font-mono text-[12px]', isSelected ? 'text-[#F8E7C9]' : 'text-emerald-700')}>
            {info.amountStr}
          </span>
        ) : (
          <span className={cn('italic text-[10px]', isSelected ? 'text-[#F8E7C9]/60' : 'text-slate-400')}>
            Amount missing
          </span>
        )}

        {info.dateStr && (
          <span className={cn('text-[10px]', isSelected ? 'text-[#F8E7C9]/70' : 'text-slate-400')}>{info.dateStr}</span>
        )}
      </div>
    </button>
  );
}

function EmptyQueueNotice({ message }: { message: string }) {
  return <p className="text-[12px] text-center p-4 text-[#064E3B]/60">{message}</p>;
}

function SectionTabs({
  sections,
  currentSection,
  onSelect,
}: {
  sections: readonly { id: string; label: string; icon: typeof ShieldAlert; badge: number }[];
  currentSection: string;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="flex gap-1 overflow-x-auto pb-1" role="tablist">
      {sections.map((section) => {
        const isActive = currentSection === section.id;
        return (
          <button
            key={section.id}
            role="tab"
            aria-selected={isActive}
            onClick={() => onSelect(section.id)}
            className={cn(
              'px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap flex items-center gap-1.5',
              isActive ? 'bg-[#064E3B] text-[#F8E7C9]' : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10'
            )}
          >
            <section.icon className="w-3.5 h-3.5" />
            {section.label}
            {section.badge > 0 && (
              <span
                className={cn(
                  'ml-1 px-1.5 py-0.5 rounded-full text-[10px] font-bold',
                  isActive ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {section.badge}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

function QueueStatus({ isLoading }: { isLoading: boolean }) {
  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-40 gap-2">
        <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
        <span className="text-xs text-[#064E3B]/50">Loading queue...</span>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-center justify-center text-center p-8 h-full opacity-60">
      <div className="w-12 h-12 rounded-full flex items-center justify-center mb-4 bg-[#064E3B]/10 text-[#064E3B]">
        <CheckCircle2 className="w-6 h-6" />
      </div>
      <h3 className="text-sm font-semibold mb-1 text-[#064E3B]">All Caught Up!</h3>
      <p className="text-[11px] text-[#064E3B]">No pending clusters requiring review.</p>
    </div>
  );
}

function QueueList({
  currentSection,
  clusters,
  unassigned,
  selectedClusterId,
  selectedUnassignedId,
  onSelectCluster,
  onSelectUnassigned,
}: {
  currentSection: string;
  clusters: ClusterRecord[];
  unassigned: UnassignedTransactionRecord[];
  selectedClusterId: string | null;
  selectedUnassignedId: string | null;
  onSelectCluster: (id: string | null) => void;
  onSelectUnassigned: (id: string | null) => void;
}) {
  if (currentSection === 'clusters') {
    if (clusters.length === 0) return <EmptyQueueNotice message="No pending clusters." />;
    return (
      <div className="flex flex-col gap-1.5">
        {clusters.map((cluster) => (
          <ClusterListItem
            key={cluster.id}
            cluster={cluster}
            isSelected={selectedClusterId === cluster.id}
            onSelect={() => onSelectCluster(selectedClusterId === cluster.id ? null : cluster.id)}
          />
        ))}
      </div>
    );
  }

  if (currentSection !== 'unassigned') return null;
  if (unassigned.length === 0) return <EmptyQueueNotice message="No unassigned transactions." />;
  return (
    <div className="flex flex-col gap-1.5">
      {unassigned.map((item) => (
        <UnassignedListItem
          key={item.id}
          item={item}
          isSelected={selectedUnassignedId === item.id}
          onSelect={() => onSelectUnassigned(selectedUnassignedId === item.id ? null : item.id)}
        />
      ))}
    </div>
  );
}

function InspectorPane({
  currentSection,
  clusters,
  selectedCluster,
  selectedUnassigned,
  selectedClusterId,
  selectedUnassignedId,
  onSelectCluster,
  onSelectUnassigned,
}: {
  currentSection: string;
  clusters: ClusterRecord[];
  selectedCluster: ClusterRecord | undefined;
  selectedUnassigned: UnassignedTransactionRecord | undefined;
  selectedClusterId: string | null;
  selectedUnassignedId: string | null;
  onSelectCluster: (id: string | null) => void;
  onSelectUnassigned: (id: string | null) => void;
}) {
  if (selectedClusterId && currentSection === 'clusters') {
    return (
      <div className="w-full h-full flex flex-col">
        <ReconciliationInspector
          cluster={selectedCluster}
          onClose={() => onSelectCluster(null)}
          inline={true}
          queueClusters={clusters}
          onNavigate={onSelectCluster}
        />
      </div>
    );
  }

  if (selectedUnassignedId && currentSection === 'unassigned') {
    return (
      <div className="w-full h-full flex flex-col">
        <UnassignedInspector record={selectedUnassigned} onClose={() => onSelectUnassigned(null)} inline={true} />
      </div>
    );
  }

  return <InspectorPlaceholder currentSection={currentSection} />;
}

function InspectorPlaceholder({ currentSection }: { currentSection: string }) {
  return (
    <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
      <div className="w-12 h-12 border-2 border-[#064E3B] rounded-xl mb-4 border-dashed flex items-center justify-center">
        <Layers className="w-6 h-6 text-[#064E3B]" />
      </div>
      <p className="text-[#064E3B] font-medium text-sm">
        {currentSection === 'clusters' ? 'Select a cluster to resolve' : 'Select an item to view details'}
      </p>
    </div>
  );
}

export default function Reconciliation() {
  const [searchParams, setSearchParams] = useSearchParams();
  const currentSection = searchParams.get('section') || 'clusters';
  const setSection = (section: string) => setSearchParams({ section });

  const { data: clusters = [], isLoading: clustersLoading } = useReconciliationClusters();
  const { data: unassigned = [], isLoading: unassignedLoading } = useUnassignedTransactions();

  const [selectedClusterId, setSelectedClusterId] = useState<string | null>(null);
  const [selectedUnassignedId, setSelectedUnassignedId] = useState<string | null>(null);

  const isLoading = clustersLoading || unassignedLoading;
  const allCaughtUp = !isLoading && clusters.length === 0 && unassigned.length === 0;

  const selectedCluster = useMemo(
    () => clusters.find((c) => c.id === selectedClusterId),
    [clusters, selectedClusterId]
  );

  const selectedUnassigned = useMemo(
    () => unassigned.find((u) => u.id === selectedUnassignedId),
    [unassigned, selectedUnassignedId]
  );

  const SECTIONS = [
    { id: 'clusters', label: 'Pending Clusters', icon: ShieldAlert, badge: clusters.length },
    { id: 'unassigned', label: 'Unassigned', icon: HelpCircle, badge: unassigned.length },
  ] as const;

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Master List (Reconciliation) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        {/* Header bar */}
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
          <div className="flex items-center justify-between">
            <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">
              Reconciliation
            </h1>
          </div>

          <SectionTabs sections={SECTIONS} currentSection={currentSection} onSelect={setSection} />
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto px-1 py-2">
          {isLoading || allCaughtUp ? (
            <QueueStatus isLoading={isLoading} />
          ) : (
            <div>
              <QueueList
                currentSection={currentSection}
                clusters={clusters}
                unassigned={unassigned}
                selectedClusterId={selectedClusterId}
                selectedUnassignedId={selectedUnassignedId}
                onSelectCluster={setSelectedClusterId}
                onSelectUnassigned={setSelectedUnassignedId}
              />
            </div>
          )}
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ─────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col justify-center">
        <InspectorPane
          currentSection={currentSection}
          clusters={clusters}
          selectedCluster={selectedCluster}
          selectedUnassigned={selectedUnassigned}
          selectedClusterId={selectedClusterId}
          selectedUnassignedId={selectedUnassignedId}
          onSelectCluster={setSelectedClusterId}
          onSelectUnassigned={setSelectedUnassignedId}
        />
      </div>
    </div>
  );
}
