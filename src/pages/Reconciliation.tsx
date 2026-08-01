import { useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { ShieldAlert, HelpCircle, Loader2, CheckCircle2, Layers } from 'lucide-react';

import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useUnassignedTransactions } from '@/hooks/queries/useUnassignedTransactions';
import ReconciliationInspector from '@/components/reconciliation/ReconciliationInspector';
import UnassignedInspector from '@/components/reconciliation/UnassignedInspector';
import { cn, formatRelativeDate, isOlderThanDays } from '@/lib/utils';
import { cleanTextForReader } from '@/components/common/GmailEmailViewer';

function getUnassignedDisplayInfo(item: any) {
  let name = item.merchant_raw || '';
  let subject = '';
  let rawHtml = '';
  let rawText = item.body_snippet || '';

  if (item.raw_payload_json) {
    try {
      const parsed = JSON.parse(item.raw_payload_json);
      if (parsed.sender || parsed.from) name = parsed.sender || parsed.from;
      if (parsed.subject) subject = parsed.subject;
      if (parsed.html) rawHtml = parsed.html;
      if (parsed.body) rawText = parsed.body;
    } catch {}
  }

  // Clean RFC 822 name (e.g. "IndusInd Bank <alerts@indusind.com>" -> "IndusInd Bank")
  if (name.includes('<')) {
    name = name.split('<')[0].replace(/^["']|["']$/g, '').trim();
  }

  // Clean text for reader view to strip all raw CSS blocks and HTML tags
  let cleanedTextSnippet = cleanTextForReader(rawHtml, rawText);

  if (!name || name === 'Bank Alert' || name === 'Bank / Service Alert') {
    if (/HDFC/i.test(cleanedTextSnippet)) name = 'HDFC Bank';
    else if (/IndusInd/i.test(cleanedTextSnippet)) name = 'IndusInd Bank';
    else if (/ICICI/i.test(cleanedTextSnippet)) name = 'ICICI Bank';
    else if (/Axis/i.test(cleanedTextSnippet)) name = 'Axis Bank';
    else if (/SBI/i.test(cleanedTextSnippet)) name = 'SBI Bank';
    else if (/Kotak/i.test(cleanedTextSnippet)) name = 'Kotak Bank';
    else name = 'Bank Alert';
  }

  // If snippet starts with repeated bank name (e.g. "IndusInd Bank IndusInd Bank ..."), trim it
  if (name && cleanedTextSnippet.startsWith(name)) {
    cleanedTextSnippet = cleanedTextSnippet.slice(name.length).trim();
  }

  const amountStr = item.amount_minor != null 
    ? `₹${(item.amount_minor / 100).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
    : null;

  const snippet = subject || cleanedTextSnippet || 'Transaction alert details missing';

  const dateStr = item.event_time 
    ? new Date(item.event_time).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
    : '';

  const issueLabel = item.reason === 'extraction_failed'
    ? 'Missing Fields'
    : item.reason === 'issuer_name_not_found'
      ? 'Unknown Card/Bank'
      : 'Action Needed';

  const avatarLetter = name.charAt(0).toUpperCase();

  return {
    name,
    snippet,
    amountStr,
    dateStr,
    issueLabel,
    avatarLetter,
  };
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

          <div className="flex gap-1 overflow-x-auto pb-1" role="tablist">
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                role="tab"
                aria-selected={currentSection === s.id}
                onClick={() => setSection(s.id)}
                className={cn(
                  'px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap flex items-center gap-1.5',
                  currentSection === s.id
                    ? 'bg-[#064E3B] text-[#F8E7C9]'
                    : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10'
                )}
              >
                <s.icon className="w-3.5 h-3.5" />
                {s.label}
                {s.badge > 0 && (
                  <span
                    className={cn(
                      'ml-1 px-1.5 py-0.5 rounded-full text-[10px] font-bold',
                      currentSection === s.id
                        ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
                        : 'bg-[#064E3B]/10 text-[#064E3B]'
                    )}
                  >
                    {s.badge}
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto px-1 py-2">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading queue...</span>
            </div>
          ) : allCaughtUp ? (
            <div className="flex flex-col items-center justify-center text-center p-8 h-full opacity-60">
              <div className="w-12 h-12 rounded-full flex items-center justify-center mb-4 bg-[#064E3B]/10 text-[#064E3B]">
                <CheckCircle2 className="w-6 h-6" />
              </div>
              <h3 className="text-sm font-semibold mb-1 text-[#064E3B]">All Caught Up!</h3>
              <p className="text-[11px] text-[#064E3B]">No pending clusters requiring review.</p>
            </div>
          ) : (
            <div>
              {currentSection === 'clusters' &&
                (clusters.length === 0 ? (
                  <p className="text-[12px] text-center p-4 text-[#064E3B]/60">
                    No pending clusters.
                  </p>
                ) : (
                  <div className="flex flex-col gap-1.5">
                    {clusters.map((cluster) => {
                      const isSelected = selectedClusterId === cluster.id;
                      const incoming = cluster.members.find((m) => m.member_role === 'incoming');
                      const merchant = incoming?.merchant || 'Match requires review';
                      const amountStr = incoming
                        ? `${incoming.direction === 'debit' ? '-' : '+'} ₹${Math.abs(incoming.amount).toFixed(2)}`
                        : null;
                      const ageStr = cluster.created_at ? formatRelativeDate(cluster.created_at) : null;
                      const isStale = cluster.created_at ? isOlderThanDays(cluster.created_at, 3) : false;

                      return (
                        <button
                          key={cluster.id}
                          onClick={() => setSelectedClusterId(isSelected ? null : cluster.id)}
                          className={cn(
                            'flex flex-col w-full text-left px-3.5 py-3 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border',
                            isSelected
                              ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-sm'
                              : 'bg-white/60 hover:bg-white text-[#064E3B] border-[#064E3B]/10 hover:border-[#064E3B]/20'
                          )}
                        >
                          <div className="flex items-center justify-between gap-2 mb-1.5 w-full">
                            <span
                              className={cn(
                                'text-[9px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider',
                                isSelected
                                  ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]'
                                  : 'bg-amber-100 text-amber-800 border border-amber-200'
                              )}
                            >
                              Ambiguous
                            </span>
                            <span
                              className={cn(
                                'flex items-center gap-1.5 text-[10px] font-medium shrink-0',
                                isSelected ? 'text-[#F8E7C9]/70' : 'text-slate-400'
                              )}
                            >
                              {ageStr && (
                                <span
                                  className={cn(isStale && !isSelected && 'text-red-500 font-semibold')}
                                  title={isStale ? 'Pending more than 3 days' : undefined}
                                >
                                  {ageStr}
                                </span>
                              )}
                              <span>{cluster.members_count} entries</span>
                            </span>
                          </div>
                          <p
                            className={cn(
                              'text-[13px] font-bold truncate mb-0.5',
                              isSelected ? 'text-white' : 'text-[#064E3B]'
                            )}
                          >
                            {merchant}
                          </p>
                          {amountStr && (
                            <p
                              className={cn(
                                'text-[12px] font-mono font-semibold',
                                isSelected
                                  ? 'text-[#F8E7C9]'
                                  : incoming!.direction === 'debit'
                                    ? 'text-red-700'
                                    : 'text-emerald-700'
                              )}
                            >
                              {amountStr}
                            </p>
                          )}
                        </button>
                      );
                    })}
                  </div>
                ))}

              {currentSection === 'unassigned' &&
                (unassigned.length === 0 ? (
                  <p className="text-[12px] text-center p-4 text-[#064E3B]/60">
                    No unassigned transactions.
                  </p>
                ) : (
                  <div className="flex flex-col gap-1.5">
                    {unassigned.map((item) => {
                      const isSelected = selectedUnassignedId === item.id;
                      const info = getUnassignedDisplayInfo(item);

                      return (
                        <button
                          key={item.id}
                          onClick={() => setSelectedUnassignedId(isSelected ? null : item.id)}
                          className={cn(
                            'flex flex-col w-full text-left px-3.5 py-3 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border',
                            isSelected
                              ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-sm'
                              : 'bg-white/60 hover:bg-white text-[#064E3B] border-[#064E3B]/10 hover:border-[#064E3B]/20'
                          )}
                        >
                          {/* Header Row */}
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
                                className={cn(
                                  'text-[13px] font-bold truncate tracking-tight',
                                  isSelected ? 'text-white' : 'text-slate-900'
                                )}
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

                          {/* Snippet Line */}
                          <p
                            className={cn(
                              'text-[11px] line-clamp-1 mb-2 font-normal',
                              isSelected ? 'text-[#F8E7C9]/80' : 'text-slate-600'
                            )}
                          >
                            {info.snippet}
                          </p>

                          {/* Footer Row */}
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
                              <span className={cn('text-[10px]', isSelected ? 'text-[#F8E7C9]/70' : 'text-slate-400')}>
                                {info.dateStr}
                              </span>
                            )}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                ))}
            </div>
          )}
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ─────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col justify-center">
        {selectedClusterId && currentSection === 'clusters' ? (
          <div className="w-full h-full flex flex-col">
            <ReconciliationInspector
              cluster={selectedCluster}
              onClose={() => setSelectedClusterId(null)}
              inline={true}
              queueClusters={clusters}
              onNavigate={setSelectedClusterId}
            />
          </div>
        ) : selectedUnassignedId && currentSection === 'unassigned' ? (
          <div className="w-full h-full flex flex-col">
            <UnassignedInspector
              record={selectedUnassigned}
              onClose={() => setSelectedUnassignedId(null)}
              inline={true}
            />
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
            <div className="w-12 h-12 border-2 border-[#064E3B] rounded-xl mb-4 border-dashed flex items-center justify-center">
              <Layers className="w-6 h-6 text-[#064E3B]" />
            </div>
            <p className="text-[#064E3B] font-medium text-sm">
              {currentSection === 'clusters'
                ? 'Select a cluster to resolve'
                : 'Select an item to view details'}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
