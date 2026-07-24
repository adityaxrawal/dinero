import { useState, useEffect, useMemo } from 'react';
import { ShieldCheck, Loader2, Mail, MonitorSmartphone } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { TransactionObservation } from '@/lib/ipc';
import { API } from '@/lib/ipc';
import SourcePipelineIcon from './SourcePipelineIcon';
import { evidenceDescription } from './evidenceDescription';

interface SourceEvidencePanelProps {
  transactionId: string;
  observations: TransactionObservation[];
}

/** `raw_payload_json`'s shape, written by `normalize_observation` (Rust). */
interface RawPayload {
  body?: string;
  html?: string | null;
}

function parseRawPayload(raw: string | null): RawPayload | null {
  if (!raw) return null;
  try {
    return JSON.parse(raw) as RawPayload;
  } catch {
    return null;
  }
}

/**
 * The email's own HTML/CSS, exactly as it rendered in Gmail (backend
 * already ran it through `sanitize_html_for_display` -- ammonia strips
 * `<script>`/`on*`/`javascript:`, drops `<img>`/`<a>`, and defangs CSS
 * `url()` before it ever reaches here). Rendered inside a sandboxed,
 * scriptless iframe via `srcDoc` so the email's own styles apply only
 * inside that isolated document — never bleeding into, or being bled into
 * by, the app's own CSS.
 */
function OriginalEmailFrame({ html }: { html: string }) {
  return (
    <div className="rounded-md border border-[#064E3B]/10 overflow-hidden bg-white">
      <iframe
        title="Original email as received in Gmail"
        srcDoc={html}
        sandbox=""
        className="w-full"
        style={{ height: 420, border: 'none' }}
      />
    </div>
  );
}

export default function SourceEvidencePanel({
  transactionId,
  observations,
}: SourceEvidencePanelProps) {
  const [sourceLog, setSourceLog] = useState<string | null>(null);
  const [isLoadingLog, setIsLoadingLog] = useState(false);

  useEffect(() => {
    let mounted = true;
    const fetchLog = async () => {
      // Only fetch if there is a gmail observation
      const hasGmail = observations.some((obs) => obs.source_pipeline?.includes('gmail'));
      if (!hasGmail) return;

      setIsLoadingLog(true);
      try {
        const log = await API.transactions.getSourceLog(transactionId);
        if (mounted) {
          setSourceLog(log);
        }
      } catch (err) {
        console.error('Failed to fetch source log', err);
      } finally {
        if (mounted) {
          setIsLoadingLog(false);
        }
      }
    };
    fetchLog();
    return () => {
      mounted = false;
    };
  }, [transactionId, observations]);

  // First observation carrying a sanitized original-HTML body -- there is
  // normally at most one gmail-sourced observation per transaction, but if
  // several exist (e.g. a merged email+statement match) the first with real
  // HTML wins rather than picking arbitrarily.
  const originalHtml = useMemo(() => {
    for (const obs of observations) {
      const payload = parseRawPayload(obs.raw_payload_json);
      if (payload?.html) return payload.html;
    }
    return null;
  }, [observations]);

  if (observations.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Source Evidence</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            No linked observations found for this transaction.
          </p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Source Evidence</CardTitle>
          <CardDescription>Every record that contributed to this transaction.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {observations.map((obs) => {
            const isStatementSourced = obs.source_pipeline === 'statement_pdf';
            const { label, detail } = evidenceDescription(obs.source_pipeline);
            return (
              <div
                key={obs.id}
                className="p-3 bg-secondary/50 rounded-md border-l-2 border-primary text-sm space-y-1.5"
              >
                <div className="flex items-center justify-between">
                  <p className="font-medium flex items-center gap-2">
                    <SourcePipelineIcon sourceMix={obs.source_pipeline} />
                    {label}
                  </p>
                  {obs.confidence_score !== null && (
                    <Badge variant="outline" className="text-[10px]">
                      {Math.round(obs.confidence_score * 100)}% confidence
                    </Badge>
                  )}
                </div>
                <p className="text-xs text-muted-foreground">{detail}</p>
                {obs.extraction_method && (
                  <p className="text-xs text-muted-foreground">
                    Extraction method: <span className="font-mono">{obs.extraction_method}</span>
                  </p>
                )}
                {isStatementSourced && (
                  <p className="flex items-center gap-1.5 text-xs text-emerald-700 font-medium pt-1">
                    <ShieldCheck className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
                    This value was confirmed by your bank statement.
                  </p>
                )}
              </div>
            );
          })}
        </CardContent>
      </Card>

      {/* Original email exactly as it rendered in Gmail -- its own HTML/CSS,
          not a reformatted summary. */}
      {originalHtml && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-[13px] flex items-center gap-2 text-[#064E3B]">
              <MonitorSmartphone className="w-4 h-4" />
              Original Email (as received in Gmail)
            </CardTitle>
            <CardDescription>
              The bank's actual email layout and styling, rendered from the sanitized HTML Gmail
              sent.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <OriginalEmailFrame html={originalHtml} />
          </CardContent>
        </Card>
      )}

      {/* Source Log / Email UI */}
      {(isLoadingLog || sourceLog) && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-[13px] flex items-center gap-2 text-[#064E3B]">
              <Mail className="w-4 h-4" />
              Source Email Content
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoadingLog ? (
              <div className="flex items-center justify-center py-4">
                <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/60" />
              </div>
            ) : sourceLog ? (
              <div className="bg-white rounded-md border border-[#064E3B]/10 p-3 max-h-[300px] overflow-y-auto">
                <pre className="text-[10px] font-mono whitespace-pre-wrap text-[#064E3B]/80 leading-relaxed">
                  {sourceLog}
                </pre>
              </div>
            ) : null}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
