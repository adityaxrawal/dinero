/**
 * Shared presentation specification for the Dinero architecture explorer.
 *
 * Both the in-app explorer (`src/pages/architecture/`) and the standalone
 * document (`docs/Dinero_Architecture.html`, emitted by
 * `scripts/generate-architecture-doc.ts`) render from this spec, so the two
 * surfaces cannot disagree about how the architecture looks: same palette,
 * same layer display, same node-card anatomy, same connector vocabulary, same
 * invariants and legend.
 *
 * Visual language: champagne/emerald "architecture explorer" — per-layer
 * accent colours on light cards, mono metadata + display headings, ink
 * emerald for selection. Node cards carry a role glyph (cylinder / queue /
 * cloud / shield / rack) so components read like a system-design blueprint.
 */

/**
 * UI theme tokens — the application's global design system (App.css "Global
 * Design System v2": Emerald Ink #064E3B + Champagne #F8E7C9, Inter). Both the
 * in-app explorer and the standalone document resolve their chrome colours
 * from these values, so the two surfaces render identically.
 */
export const THEME = {
  bg: '#f8e7c9', // --bg-canvas
  surface: '#ecddb0', // --bg-surface
  card: '#F3EBDD', // --bg-card
  cardHover: '#e8dbc8', // --bg-card-hover
  panel: '#F3EBDD', // --bg-panel
  ink: '#064e3b', // --emerald-ink
  inkHover: '#053d2f', // --emerald-ink-hover
  inkLight: '#0a6e53', // --emerald-ink-light
  inkSubtle: 'rgba(6, 78, 59, 0.07)', // --emerald-ink-subtle
  inkSubtleHover: 'rgba(6, 78, 59, 0.12)',
  text: '#0d2b22', // --text-primary
  text2: '#3d5a50', // --text-secondary
  text3: '#4a655c', // darker than --text-muted for WCAG AA small text
  border: '#d9c8a8', // --border-color
  borderStrong: '#c4ae88', // --border-strong
  borderSubtle: 'rgba(6, 78, 59, 0.08)', // --border-subtle
  champagne: '#f8e7c9',
  champagneDark: '#f0d4a8',
  champagneDeeper: '#e8c888',
  warning: '#f59e0b',
  warningBg: 'rgba(245, 158, 11, 0.1)',
  danger: '#ef4444',
  success: '#10b981',
} as const;

/**
 * Typography — the application's global family (Inter, per tailwind.config
 * `fontFamily.sans` and App.css). The explorer inherits it everywhere; the
 * standalone document links the same family with identical fallbacks.
 */
export const FONTS = {
  sans: "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
} as const;

/** Per-layer display: id, name, tech strapline and accent colour. */
export interface LayerDisplay {
  id: string;
  code: string;
  name: string;
  tech: string;
  color: string;
}

/**
 * Listed in canvas order (top to bottom): this array is the single source for
 * the rail order, the A–L letters and the band order, so they cannot disagree.
 */
export const LAYER_DISPLAY: LayerDisplay[] = [
  {
    id: 'external',
    code: 'A',
    name: 'Strict Network Boundaries & Externals',
    tech: 'Fail-closed audited egress',
    color: '#57534e',
  },
  {
    id: 'client',
    code: 'B',
    name: 'Client Surfaces & UI',
    tech: 'React 19 · TypeScript · Vite 7',
    color: '#0a6e53',
  },
  {
    id: 'ipc',
    code: 'C',
    name: 'Tauri IPC Bridge & Gateway',
    tech: 'Tauri v2 · Rust · Serde',
    color: '#1d4ed8',
  },
  {
    id: 'ingestion',
    code: 'D',
    name: 'Gmail Ingestion Pipeline',
    tech: 'Gmail API · OAuth2 PKCE',
    color: '#0369a1',
  },
  {
    id: 'extraction',
    code: 'E',
    name: '6-Layer Financial Extraction Ladder',
    tech: 'Regex · NLP · llama.cpp',
    color: '#7c3aed',
  },
  {
    id: 'sidecars',
    code: 'F',
    name: 'Isolated Sidecar Processes',
    tech: 'pdfium (stdio pipe) · llama.cpp (loopback HTTP)',
    color: '#4338ca',
  },
  {
    id: 'statements',
    code: 'G',
    name: 'PDF Statement Engine & Review Gate',
    tech: 'pdfium sidecar · two-phase commit',
    color: '#c2410c',
  },
  {
    id: 'queues',
    code: 'H',
    name: '4-Queue Work Distribution Engine',
    tech: 'Rust · Tokio bounded channels',
    color: '#0e7490',
  },
  {
    id: 'reconciliation',
    code: 'I',
    name: 'Deterministic Reconciliation Engine',
    tech: 'Weighted multi-signal scoring',
    color: '#15803d',
  },
  {
    id: 'licensing',
    code: 'J',
    name: 'Device Licensing & Monetization',
    tech: 'RS256 JWT · Vercel · Razorpay',
    color: '#be123c',
  },
  {
    id: 'workers',
    code: 'K',
    name: 'Background Workers & Incident Response',
    tech: 'Tokio loops · health · backup',
    color: '#a16207',
  },
  {
    id: 'persistence',
    code: 'L',
    name: 'Local-First Storage & SQLCipher',
    tech: 'SQLCipher AES-256 · Keychain',
    color: '#475569',
  },
];

export const LAYER_DISPLAY_BY_ID: Record<string, LayerDisplay> = Object.fromEntries(
  LAYER_DISPLAY.map((l) => [l.id, l])
);

/**
 * Connector vocabulary → colour + dash. Every kind has its own dash pattern as
 * well as its own colour, so kinds stay distinguishable without colour vision
 * (and in print). Mirrors the explorer legend.
 */
export const EDGE_KIND_DISPLAY: Record<string, { label: string; color: string; dash: string }> = {
  sync: { label: 'Synchronous call / data flow', color: '#0B4A3A', dash: '' },
  async: { label: 'Async queue / event stream', color: '#0E7490', dash: '8 4' },
  event: { label: 'UI notification / push event', color: '#6D28D9', dash: '2 3' },
  db: { label: 'Database read / write', color: '#475569', dash: '12 3 2 3' },
  network: { label: 'External network egress', color: '#B45309', dash: '14 3 2 3 2 3' },
  secret: { label: 'Security / key material', color: '#BE123C', dash: '3 2 3 6' },
  pipe: { label: 'Sidecar process pipe (stdio)', color: '#4338CA', dash: '1 2' },
  gate: { label: 'License / validation gate', color: '#C2410C', dash: '5 2 1 2' },
};

/** Node-card anatomy: `ID · kind` / name / role / tech chips. */
export const NODE_KIND_LABEL: Record<string, string> = {
  ui: 'client',
  service: 'service',
  command: 'boundary',
  gate: 'gate',
  queue: 'queue',
  store: 'datastore',
  process: 'process',
  external: 'external',
  worker: 'worker',
};

/** Privacy invariants shown in the rail — the system's non-negotiables. */
export const INVARIANTS: { title: string; body: string }[] = [
  {
    title: 'Invariant 1 · Offline Data Plane',
    body: 'Transactions, accounts, balances and statements never touch any external server or third-party LLM API. The licensing backend only ever sees license, device and payment state.',
  },
  {
    title: 'Invariant 2 · Keychain-Only Secrets',
    body: 'OAuth tokens, the SQLCipher database key and the PDF-password key live only in the macOS Keychain — never SQLite columns, plaintext files or env vars in release builds.',
  },
  {
    title: 'Invariant 3 · Never Guess — Block And Ask',
    body: 'The statement Instrument Gate never defaults a missing issuer/masked-ID; Gate 3 never accepts a partial extraction; a wrong PDF password re-prompts without losing context.',
  },
  {
    title: 'Invariant 4 · Fail-Closed Trust Boundaries',
    body: 'License JWT verification, PDF sidecar infrastructure errors and SPF/DKIM/DMARC checks all treat an unverifiable state as denied — never "assume OK".',
  },
  {
    title: 'Invariant 5 · Process Isolation As Security',
    body: 'pdf_sidecar and llama_sidecar run as isolated OS processes so a malformed PDF or heavy inference can never crash or compromise the main Tauri process.',
  },
];

/** Stat tiles in the rail header. Values derive from the model at render time. */
/**
 * Text of the Architecture Analysis pane. Shared so the in-app explorer and
 * the standalone document render identical wording. Inline markup is limited
 * to **bold** and `code` (see `analysisSegments`).
 */
export const ANALYSIS = {
  shape: [
    '**Dinero** is a local-first macOS personal-finance tracker: a Tauri v2 application (Rust core + React 19 frontend) that ingests bank/UPI transaction alerts and PDF statements from Gmail — or manual upload — extracts and reconciles them into one unified ledger **entirely on-device**, paired with a minimal separate cloud service that handles **only** licensing and billing.',
    'Two ingestion channels converge on three routing queues behind a single license write-gate, with a fourth queue carrying deferred LLM work downstream; every queue funnels into one reconciliation engine that produces the canonical ledger the React frontend renders. A dozen background workers keep the system self-healing without ever leaving the device.',
  ],
  patterns: [
    '**One processing path per job type** — historical scan, live polling and manual upload all converge on the same queues and the same Gate 1→2→3 / password-resolution logic.',
    '**Async acknowledgment, event-driven outcome** — expensive operations return immediately and report results via named events; the UI never blocks on multi-minute work.',
    '**Checkpoint everything long-running** — scans, Gmail history cursors, statement drafts and queued ingest jobs are durably resumable across crashes.',
    '**Precision over recall** — Gate 3 never accepts a partial extraction and holds every layer to one 0.75 confidence floor; the Instrument Gate never defaults a missing identifier; a wrong PDF password re-prompts without losing context.',
  ],
  profile: [
    {
      k: 'Queues',
      v: 'Transaction 4w/256 · Statement 5-permit/64 · Mandate 1w/64 · Layer-6 6w/256 — all durable (pending_ingest_jobs, the statements table, layer6_pending_jobs)',
    },
    {
      k: 'Workers',
      v: '12 boot-spawned loops (polling 60s, health + stale-sync watchdog 60s, maintenance daily and at launch when due, licensing 6h, updates ~6h)',
    },
    {
      k: 'Storage',
      v: 'SQLCipher SQLite — 45 tables + FTS5 search index, 68 migrations, append-only audit + match trails; daily verified backup, 7 generations',
    },
    {
      k: 'Sidecars',
      v: 'pdf_sidecar (spawn-per-file) · llama_sidecar (long-lived loopback llama-server, b10068, per-run API key, restarted when dead or hung)',
    },
    {
      k: 'IPC',
      v: '149 registered Tauri commands (141 in release builds), each needing an explicit permission; 39 writes test-enforced through assert_write_allowed',
    },
    {
      k: 'Licensing',
      v: '8 statuses; 8 local edges plus server-verified updates; signed state claim in a 7-day JWT; Locked lifts on renewal; 7-day offline grace',
    },
  ],
  gaps: [
    '**ADR-024 — NetworkClient coverage.** The updater check is the one remaining outbound path that leaves no Network Activity row. Gmail, Google OAuth, licensing and both model downloads are logged.',
    '**IPC command-count discrepancy.** The code registers 149 commands in generate_handler!; the design docs quote 143 and 151; figures are recorded rather than silently reconciled.',
    '**License state machine.** Two write paths: 8 local, time-driven edges (is_legal_transition) and server-verified updates (apply_server_validation) that can set any server state. Locked has no local exit but a renewal lifts it at the next validation.',
    '**Statement row extraction.** All seven bank-specific parsers matched zero of nine real statements; `parse_universal` took the corpus 0→79 rows.',
    '**Live polling rate limits.** history.list retries 429 and 5xx but not a 403 rateLimitExceeded: that poll cycle fails and the next one, 60s later, retries. The historical scan retries both.',
  ],
  help: 'Click any component to inspect it. Use **Impact** for the downstream blast radius, **Labels** to pin connector labels, and the workflow rail to step through an end-to-end flow — the participating components light up as you go.',
};

/** Splits analysis text into plain, **bold** and `code` segments. */
export function analysisSegments(text: string): { kind: 'text' | 'b' | 'code'; text: string }[] {
  return text
    .split(/(\*\*[^*]+\*\*|`[^`]+`)/)
    .filter(Boolean)
    .map((part) =>
      part.startsWith('**')
        ? { kind: 'b' as const, text: part.slice(2, -2) }
        : part.startsWith('`')
          ? { kind: 'code' as const, text: part.slice(1, -1) }
          : { kind: 'text' as const, text: part }
    );
}

export const STAT_DEFS = [
  { key: 'components', label: 'Components' },
  { key: 'connections', label: 'Connections' },
  { key: 'layers', label: 'Layers' },
  { key: 'steps', label: 'Workflow steps' },
] as const;

/**
 * Short display code shown at the top of a node card (`N4 · external`).
 * Shared by the in-app explorer and the standalone document so both render
 * identical card headers. Codes are unique per component: the model registers
 * its id order once via `setNodeCodeOrder` (see data/index.ts).
 */
const NODE_CODE: Record<string, string> = {};

export function setNodeCodeOrder(ids: string[]): void {
  ids.forEach((id, i) => {
    NODE_CODE[id] = `N${i + 1}`;
  });
}

export function nodeCode(id: string): string {
  return NODE_CODE[id] ?? id.split('-')[0].slice(0, 2).toUpperCase();
}

/** The one-line kind label used in the node-card id line. */
export function nodeKindLabel(kind: string): string {
  return NODE_KIND_LABEL[kind] ?? kind;
}

/* ─────────────────────── graph geometry (shared) ─────────────────────── */

/** Node-card and cluster-box geometry, shared by the in-app layout and the
 *  standalone document so both place components identically. */
export const NODE_W = 248;
// Fits the card anatomy (id line + up to 2-line name + 2-line role + chip row)
// without clipping — previously 86, which cut into the content stack.
export const NODE_H = 106;
export const BAND_PAD_TOP = 44;
export const BAND_PAD_X = 16;
export const BAND_GAP = 14;
export const BAND_V_GAP = 34;
export const NODE_V_GAP = 16;
export const MAX_BAND_W = 1420;

/** Vertical order of layer bands — the request lifecycle, top to bottom. */
export const BAND_ORDER: string[] = LAYER_DISPLAY.map((l) => l.id);
