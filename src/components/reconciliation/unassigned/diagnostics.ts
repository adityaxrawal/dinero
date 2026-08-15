/**
 * Derives the failure diagnosis from which extraction signals are absent.
 */
import type { UnassignedTransactionRecord } from '@/lib/ipc';

// Backend failure reasons mapped to human titles. Keyed by the reason codes the
// reconciliation pipeline emits.
const REASON_TITLE: Record<string, string> = {
  extraction_failed: 'Failed to Extract Details',
  issuer_name_not_found: 'Unknown Payment Instrument',
  pending_llm_enrichment: 'Pending Background AI Enrichment',
  'gate3_failed:missing_amount': 'Missing Transaction Amount',
  'gate3_failed:missing_counterparty': 'Missing Counterparty / Merchant',
  'gate3_failed:missing_instrument': 'Missing Instrument Details',
  gate3_failed: 'Mandatory Field Gate Failed',
};

/** Human title for a failure reason, falling back for unrecognised codes. */
export function reasonTitle(reason: string): string {
  return REASON_TITLE[reason] || reason || 'Unresolved Issue';
}

export interface ReasonGuidance {
  description: string;
  tip: string;
}

// Used when a reason has no specific guidance, so the panel always tells the
// user something actionable rather than rendering blank.
const DEFAULT_GUIDANCE: ReasonGuidance = {
  description: 'An unexpected issue occurred while processing this message.',
  tip: 'Review the extracted data below and complete any missing fields before saving.',
};

// Per-reason explanation and suggested fix. Written as guidance the user can
// act on, not as a restatement of the error.
const REASON_GUIDANCE: Record<string, ReasonGuidance> = {
  extraction_failed: {
    description:
      "Dinero's rule engine could not reliably parse the minimum required transaction details (amount, merchant, or date) from this message text.",
    tip: 'Fill in the missing fields manually using the extracted data form below or Quick-Fill buttons from the email evidence.',
  },
  issuer_name_not_found: {
    description:
      'Transaction details were parsed successfully, but Dinero could not match the bank or card mentioned in the email to any configured payment instrument in your settings.',
    tip: 'Select an existing instrument from the dropdown below, or go to Settings → Payment Instruments to add this new card/account.',
  },
  pending_llm_enrichment: {
    description:
      'Rule-based parsers could not confidently extract all details. An AI LLM enrichment task has been enqueued to extract missing fields.',
    tip: 'You can wait for the background AI worker to complete or manually input the fields below and save.',
  },
  'gate3_failed:missing_amount': {
    description:
      'Merchant name and date were identified, but no valid transaction amount figure could be parsed from the email body.',
    tip: 'Look at the email text in Source Email Evidence below and enter the amount in the form.',
  },
  'gate3_failed:missing_counterparty': {
    description:
      'Transaction amount and date were found, but no counterparty or merchant name could be extracted, or candidate was rejected as generic boilerplate.',
    tip: 'Type the merchant name into the Merchant field below or use Quick-Fill.',
  },
  'gate3_failed:missing_instrument': {
    description:
      'Amount, merchant, and date were extracted, but no bank name, card last-4 digits, or UPI ID signal was present in the email.',
    tip: 'Select the payment instrument used for this transaction from the Instrument dropdown below.',
  },
};

/** Guidance for a failure reason, with a usable default. */
export function reasonGuidance(reason: string): ReasonGuidance {
  return REASON_GUIDANCE[reason] ?? DEFAULT_GUIDANCE;
}

const RED = 'bg-red-500/10 text-red-700 border-red-200';
const EMERALD = 'bg-emerald-500/10 text-emerald-800 border-emerald-300';
const AMBER = 'bg-amber-500/10 text-amber-800 border-amber-300';

export interface ExtractionBadge {
  engineLabel: string;
  confidenceLabel: string;
  confidenceBadgeStyle: string;
}

/**
 * Summarises how complete an extraction was, as a coloured badge.
 *
 * Colour encodes severity so the queue can be triaged visually: what is missing
 * determines whether a row is trivially fixable or genuinely ambiguous.
 */
export function extractionBadge(record: UnassignedTransactionRecord): ExtractionBadge {
  const method = record.extraction_method;

  if (method?.startsWith('regex_')) {
    return {
      engineLabel: `Deterministic (${method})`,
      confidenceLabel: '100% (Rule Match)',
      confidenceBadgeStyle: EMERALD,
    };
  }

  if (method === 'llm_layer6') {
    const engineLabel = 'AI Layer 6 LLM';
    if (record.confidence_score == null) {
      return { engineLabel, confidenceLabel: 'AI Extracted (Medium)', confidenceBadgeStyle: AMBER };
    }
    const pct = Math.round(record.confidence_score * 100);
    return {
      engineLabel,
      confidenceLabel: `${pct}% Confidence`,
      confidenceBadgeStyle: pct >= 80 ? EMERALD : pct >= 50 ? AMBER : RED,
    };
  }

  if (record.reason === 'pending_llm_enrichment') {
    return {
      engineLabel: 'AI Layer 6 LLM (Pending)',
      confidenceLabel: 'Enqueued for AI',
      confidenceBadgeStyle: AMBER,
    };
  }

  if (record.confidence_score != null) {
    const pct = Math.round(record.confidence_score * 100);
    return {
      engineLabel: 'Standard Ladder',
      confidenceLabel: `${pct}% Confidence`,
      confidenceBadgeStyle: pct >= 80 ? EMERALD : AMBER,
    };
  }

  return {
    engineLabel: 'Standard Ladder',
    confidenceLabel: '0% (Unextracted)',
    confidenceBadgeStyle: RED,
  };
}

export interface DiagnosticCheck {
  id: string;
  label: string;
  passed: boolean;
  value: string;
}

/**
 * Diagnoses specifically why instrument attribution failed.
 *
 * Separated from the other checks because it is the most common cause and has
 * distinct sub-cases -- no signal extracted at all, versus a signal that matched
 * no known instrument.
 */
function instrumentCheck(record: UnassignedTransactionRecord): DiagnosticCheck {
  const unmatched = record.reason === 'issuer_name_not_found';
  const noSignal = record.reason.includes('missing_instrument');
  const extractionFailed = record.reason === 'extraction_failed';
  const value = unmatched
    ? 'Card/Account Unmatched in Settings'
    : noSignal
      ? 'No Card/Account Identifier in Email'
      : extractionFailed
        ? 'Missing / Unparsed'
        : 'Instrument Signal Matched';
  return {
    id: 'instrument',
    label: 'Instrument Resolution (Gate 4)',
    passed: !unmatched && !noSignal && !extractionFailed,
    value,
  };
}

/**
 * Builds the diagnostic checklist for an unassigned transaction.
 *
 * Reports which signals were recovered and which were missing, so the user sees
 * why attribution failed rather than only that it did.
 */
export function buildChecks(record: UnassignedTransactionRecord): DiagnosticCheck[] {
  const hasAmount = record.amount_minor != null;
  return [
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
      passed: hasAmount,
      value: hasAmount
        ? `${record.currency ?? 'INR'} ${(record.amount_minor! / 100).toFixed(2)}`
        : 'Missing / Unparsed',
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
    instrumentCheck(record),
  ];
}

export interface EmailEvidence {
  html: string;
  text: string;
  subject: string;
  sender: string;
}

/**
 * Extracts the source email from the record's raw payload, for display.
 *
 * Tolerates a malformed or absent payload by returning empty fields, since the
 * evidence pane is supplementary and must not break the resolver around it.
 */
export function parseEmailEvidence(record: UnassignedTransactionRecord): EmailEvidence {
  const empty: EmailEvidence = { html: '', text: '', subject: '', sender: '' };

  if (!record.raw_payload_json) {
    return { ...empty, text: record.body_snippet || '' };
  }
  try {
    const parsed = JSON.parse(record.raw_payload_json);
    return {
      html: parsed.html || '',
      text: parsed.body || parsed.snippet || '',
      subject: parsed.subject || '',
      sender: parsed.sender || parsed.from || '',
    };
  } catch {
    return { ...empty, text: record.body_snippet || '' };
  }
}
