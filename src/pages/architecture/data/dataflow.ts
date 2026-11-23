/**
 * Data-flow diagram definition — the master source → gates → queues → ledger
 * pipeline (docs/design/00 §4) with its parked/discarded branches and the
 * seven traceable end-to-end paths.
 *
 * Lives in the shared model so the in-app explorer and the standalone
 * `docs/Dinero_Architecture.html` render exactly the same diagram.
 */

export type StageId =
  | 'gmail'
  | 'gate1'
  | 'gate2'
  | 'gate3'
  | 'drop1'
  | 'drop2'
  | 'l6'
  | 'obs'
  | 'unassigned'
  | 'upload'
  | 'pw'
  | 'awaitpw'
  | 'parse'
  | 'awaitinst'
  | 'draft'
  | 'commit'
  | 'mandate'
  | 'recurring'
  | 'recon'
  | 'ledger'
  | 'ui';

export interface Stage {
  id: StageId;
  label: string;
  sub?: string;
  color: string;
  shape: 'process' | 'store' | 'entity' | 'terminator' | 'decision';
  x: number;
  y: number;
  /** Components implementing this stage. */
  components: string[];
  detail: string;
}

export const DF_COLORS = {
  gate: '#c2410c',
  flow: '#0369a1',
  queue: '#0e7490',
  drop: '#a8a29e',
  good: '#15803d',
  ai: '#7c3aed',
  user: '#0a6e53',
};

export const STAGES: Stage[] = [
  {
    id: 'gmail',
    label: 'Gmail message',
    color: DF_COLORS.flow,
    shape: 'terminator',
    x: 0,
    y: 260,
    components: ['gmail-client', 'gmail-api'],
    detail:
      'Live polling (history.list) or historical scan fetches a message. Manual PDF upload joins at the statement path.',
  },
  {
    id: 'gate1',
    label: 'Gate 1',
    sub: 'sender verify',
    color: DF_COLORS.gate,
    shape: 'decision',
    x: 220,
    y: 260,
    components: ['message-processor'],
    detail:
      'Registry match + homoglyph/punycode/typo-squat detection + SPF/DKIM/DMARC cross-check (downgrade-only).',
  },
  {
    id: 'drop1',
    label: 'Discarded',
    sub: 'reject / spoof',
    color: DF_COLORS.drop,
    shape: 'store',
    x: 220,
    y: 130,
    components: ['message-processor'],
    detail: 'Noise, unverified-without-history and spoofed senders are discarded (audited).',
  },
  {
    id: 'gate2',
    label: 'Gate 2',
    sub: 'classify',
    color: DF_COLORS.gate,
    shape: 'decision',
    x: 460,
    y: 260,
    components: ['message-processor'],
    detail: 'Classify as transaction / statement / mandate / noise / otp / kyc / marketing.',
  },
  {
    id: 'drop2',
    label: 'Ignored / Audited',
    sub: 'noise · otp · kyc',
    color: DF_COLORS.drop,
    shape: 'store',
    x: 460,
    y: 60,
    components: ['message-processor'],
    detail:
      'Noise goes to Ignored Messages (recoverable); otp/kyc/marketing go to the Audit Log (hard reject).',
  },
  {
    id: 'gate3',
    label: 'Gate 3',
    sub: 'extract (6-layer ladder)',
    color: DF_COLORS.gate,
    shape: 'decision',
    x: 700,
    y: 380,
    components: ['extraction-ladder', 'learning', 'merchant-norm', 'fingerprint'],
    detail:
      'Learned rules → bank template → generic regex → NLP → statement cross-ref, then a mandatory-field gate and one confidence floor (0.75) for every layer.',
  },
  {
    id: 'l6',
    label: 'Layer 6 queue',
    sub: 'LLM, durable',
    color: DF_COLORS.ai,
    shape: 'store',
    x: 700,
    y: 540,
    components: ['layer6-queue', 'llm-manager', 'llama-sidecar'],
    detail:
      'When Layers 1–5 fail on an LLM-eligible machine (enough RAM and a downloaded model), the job is deferred here. Jobs survive restart (layer6_pending_jobs); a fixed pool of 6 workers runs them, at most 3 attempts each.',
  },
  {
    id: 'obs',
    label: 'Observation',
    sub: 'normalized + fingerprint',
    color: DF_COLORS.flow,
    shape: 'store',
    x: 960,
    y: 380,
    components: ['tx-queue', 'fingerprint', 'merchant-norm'],
    detail:
      'Transaction Queue workers normalize, fingerprint and insert the observation idempotently. Jobs from Gmail are persisted (pending_ingest_jobs) until this succeeds.',
  },
  {
    id: 'unassigned',
    label: 'Unassigned queue',
    sub: 'extraction failed',
    color: DF_COLORS.drop,
    shape: 'store',
    x: 960,
    y: 560,
    components: ['sqlite'],
    detail:
      'Partial data is salvaged into unassigned_transactions for user resolution; results below the 0.75 confidence floor land here too.',
  },
  {
    id: 'upload',
    label: 'Manual PDF',
    sub: 'or email attachment',
    color: DF_COLORS.user,
    shape: 'terminator',
    x: 0,
    y: 60,
    components: ['statement-pipeline', 'react-ui'],
    detail: 'Both entry points converge on the same password resolution and staging pipeline.',
  },
  {
    id: 'pw',
    label: 'Password resolve',
    sub: 'never blocks',
    color: DF_COLORS.gate,
    shape: 'terminator',
    x: 220,
    y: 60,
    components: ['statement-pipeline', 'pdf-storage', 'pdf-sidecar'],
    detail:
      'is_pdf_unencrypted → try_all_stored_passwords → PromptCreated. Awaiting-password is parked, not blocking.',
  },
  {
    id: 'awaitpw',
    label: 'Awaiting password',
    sub: 'parked',
    color: DF_COLORS.drop,
    shape: 'store',
    x: 220,
    y: -60,
    components: ['sqlite'],
    detail:
      'unprocessed_statements row; re-parse later via statements_retry_unprocessed / statements_reparse_all.',
  },
  {
    id: 'parse',
    label: 'Parse + Instrument Gate',
    sub: 'sidecar pdfium',
    color: DF_COLORS.gate,
    shape: 'decision',
    x: 460,
    y: -20,
    components: ['statement-pipeline', 'pdf-sidecar', 'instrument-gate', 'row-extractor'],
    detail:
      'Store encrypted PDF → extract text → metadata → Instrument Gate → duplicate check → rows. Issuer + masked-ID are mandatory.',
  },
  {
    id: 'awaitinst',
    label: 'Awaiting instrument',
    sub: 'never guessed',
    color: DF_COLORS.drop,
    shape: 'store',
    x: 460,
    y: -180,
    components: ['instrument-gate'],
    detail:
      'Missing issuer/masked-ID parks the statement and emits INSTRUMENT_CONFIRMATION_REQUIRED.',
  },
  {
    id: 'draft',
    label: 'Draft',
    sub: 'pending review',
    color: DF_COLORS.user,
    shape: 'store',
    x: 700,
    y: -20,
    components: ['statement-pipeline', 'react-ui'],
    detail:
      'statement_drafts row with status pending_review; the user can edit rows and metadata before anything becomes real.',
  },
  {
    id: 'commit',
    label: 'Commit',
    sub: 'explicit user action',
    color: DF_COLORS.good,
    shape: 'terminator',
    x: 900,
    y: -20,
    components: ['statement-pipeline', 'react-ui'],
    detail:
      'commit_staged_draft writes statements + statement_entries, builds observations and reconciles each row.',
  },
  {
    id: 'mandate',
    label: 'Mandate extract',
    sub: 'auto-pay lifecycle',
    color: DF_COLORS.flow,
    shape: 'process',
    x: 700,
    y: 180,
    components: ['mandate-recurring'],
    detail: 'Mandate registration/cancellation events; missing fields are rejected.',
  },
  {
    id: 'recurring',
    label: 'Recurring payment',
    sub: 'marker, no ledger entry',
    color: DF_COLORS.flow,
    shape: 'process',
    x: 900,
    y: 180,
    components: ['mandate-recurring', 'mandate-queue'],
    detail:
      'The Mandate Queue upserts recurring_payments and records the decision in processed_messages. Mandates are markers only: no transaction is booked in the ledger.',
  },
  {
    id: 'recon',
    label: 'Reconcile',
    sub: 'prefilter → exact → scored → cluster',
    color: DF_COLORS.good,
    shape: 'process',
    x: 1200,
    y: 300,
    components: ['recon-engine'],
    detail:
      'Ambiguous matches become clusters for human review; the engine refuses to auto-resolve them. match_decisions is append-only.',
  },
  {
    id: 'ledger',
    label: 'Ledger',
    sub: 'canonical transactions',
    color: DF_COLORS.good,
    shape: 'store',
    x: 1440,
    y: 300,
    components: ['sqlite'],
    detail: 'SQLCipher SQLite — the canonical, local-only ledger.',
  },
  {
    id: 'ui',
    label: 'React UI',
    sub: 'dashboard updates',
    color: DF_COLORS.user,
    shape: 'terminator',
    x: 1660,
    y: 300,
    components: ['react-ui', 'events'],
    detail:
      'transaction_created / statement_staged events invalidate React Query caches and update the views.',
  },
];

export interface FlowEdge {
  from: StageId;
  to: StageId;
  label: string;
  path: StageId[]; // full linear path this edge belongs to, for highlighting
}

/** Paths through the graph; picking a stage highlights every path containing it. */
export const PATHS: { id: string; name: string; stages: StageId[] }[] = [
  {
    id: 'p-tx',
    name: 'Transaction alert → ledger',
    stages: ['gmail', 'gate1', 'gate2', 'gate3', 'obs', 'recon', 'ledger', 'ui'],
  },
  {
    id: 'p-l6',
    name: 'Ladder exhausted → Layer 6 → ledger',
    stages: ['gmail', 'gate1', 'gate2', 'gate3', 'l6', 'obs', 'recon', 'ledger', 'ui'],
  },
  {
    id: 'p-fail',
    name: 'Extraction failed → unassigned',
    stages: ['gmail', 'gate1', 'gate2', 'gate3', 'unassigned'],
  },
  {
    id: 'p-stmt',
    name: 'Statement → reviewed → committed',
    stages: [
      'gmail',
      'gate1',
      'gate2',
      'pw',
      'parse',
      'draft',
      'commit',
      'obs',
      'recon',
      'ledger',
      'ui',
    ],
  },
  {
    id: 'p-upload',
    name: 'Manual upload → committed',
    stages: ['upload', 'pw', 'parse', 'draft', 'commit', 'obs', 'recon', 'ledger', 'ui'],
  },
  {
    id: 'p-mandate',
    name: 'Mandate → recurring payment (no ledger entry)',
    stages: ['gmail', 'gate1', 'gate2', 'mandate', 'recurring'],
  },
  {
    id: 'p-park',
    name: 'Parked (awaiting password / instrument)',
    stages: ['upload', 'pw', 'awaitpw', 'parse', 'awaitinst'],
  },
];

export const EDGES: FlowEdge[] = [
  { from: 'gmail', to: 'gate1', label: 'message', path: [] },
  { from: 'gate1', to: 'drop1', label: 'reject', path: [] },
  { from: 'gate1', to: 'gate2', label: 'pass', path: [] },
  { from: 'gate2', to: 'drop2', label: 'noise / otp', path: [] },
  { from: 'gate2', to: 'gate3', label: 'transaction', path: [] },
  { from: 'gate2', to: 'pw', label: 'statement', path: [] },
  { from: 'gate2', to: 'mandate', label: 'mandate', path: [] },
  { from: 'gate3', to: 'obs', label: 'pass', path: [] },
  { from: 'gate3', to: 'l6', label: 'defer', path: [] },
  { from: 'gate3', to: 'unassigned', label: 'fail', path: [] },
  { from: 'l6', to: 'obs', label: 'pass', path: [] },
  { from: 'l6', to: 'unassigned', label: 'fail', path: [] },
  { from: 'upload', to: 'pw', label: 'pdf', path: [] },
  { from: 'pw', to: 'awaitpw', label: 'parked', path: [] },
  { from: 'pw', to: 'parse', label: 'ok', path: [] },
  { from: 'parse', to: 'awaitinst', label: 'blocked', path: [] },
  { from: 'parse', to: 'draft', label: 'staged', path: [] },
  { from: 'draft', to: 'commit', label: 'commit', path: [] },
  { from: 'commit', to: 'obs', label: 'observations', path: [] },
  { from: 'mandate', to: 'recurring', label: 'extracted', path: [] },
  { from: 'obs', to: 'recon', label: 'reconcile', path: [] },
  { from: 'recon', to: 'ledger', label: 'canonical', path: [] },
  { from: 'ledger', to: 'ui', label: 'events', path: [] },
];
