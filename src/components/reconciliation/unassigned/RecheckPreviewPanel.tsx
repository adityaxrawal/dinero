/**
 * What a re-check would do, shown before anything is booked (current-issues
 * #3): the values the rules found, which rule layer found them, and either
 * "Book it" or why it would stay in review, with "Ask the AI now".
 */
import { Button } from '@/components/ui/button';
import { formatMoney } from '@/lib/formatMoney';
import type { RecheckPreview } from '@/lib/ipc';

/** Plain names for the extraction layers. */
const LAYER_LABEL: Record<string, string> = {
  learned_fields: 'your learned patterns',
  bank_templates: 'the bank template',
  generic_regex: 'general patterns',
  nlp: 'text analysis',
};

export default function RecheckPreviewPanel({
  preview,
  busy,
  onBook,
  onAskAi,
  onClose,
}: {
  preview: RecheckPreview;
  busy: boolean;
  onBook: () => void;
  onAskAi: () => void;
  onClose: () => void;
}) {
  const source = preview.layer ? (LAYER_LABEL[preview.layer] ?? preview.layer) : null;
  const card = preview.card_from_rule
    ? 'From your card rule'
    : preview.card_last4
      ? `${preview.card_issuer ?? 'Card'} •• ${preview.card_last4}`
      : '—';
  const rows: [string, string][] = [
    ['Merchant', preview.merchant ?? '—'],
    [
      'Amount',
      preview.amount_minor == null
        ? '—'
        : formatMoney(preview.amount_minor, preview.currency, preview.direction),
    ],
    ['Card or account', card],
  ];

  return (
    <section
      aria-label="Re-check preview"
      className="rounded-lg border border-emerald-ink/15 bg-emerald-ink/5 p-3 text-[12px] text-emerald-ink space-y-2"
    >
      <p className="font-semibold">
        {source ? `Found by ${source}` : 'The rules found nothing new in this email'}
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
        {rows.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-emerald-ink/85">{label}</dt>
            <dd className="font-medium">{value}</dd>
          </div>
        ))}
      </dl>
      {preview.ai_queued ? (
        <p role="status">The local AI already has this email. Ask the AI now moves it up the queue.</p>
      ) : preview.would_book ? (
        <p role="status">Booking would add this to your ledger.</p>
      ) : (
        <p role="status">It would stay in review: {preview.missing ?? 'details are missing'}.</p>
      )}
      <div className="flex gap-2 pt-1">
        {preview.would_book && (
          <Button size="sm" onClick={onBook} disabled={busy}>
            Book it
          </Button>
        )}
        {!preview.would_book && (
          <Button size="sm" variant="outline" onClick={onAskAi} disabled={busy}>
            Ask the AI now
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={onClose} disabled={busy}>
          Close
        </Button>
      </div>
    </section>
  );
}
