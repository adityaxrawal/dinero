/**
 * Layer definitions for the architecture map.
 *
 * Colours are muted, print-safe accents that read clearly on the app's
 * champagne canvas; each layer also carries a soft tint used for the
 * grouping box behind its components.
 */
import type { LayerDef } from './types';

export const LAYERS: LayerDef[] = [
  {
    id: 'client',
    label: 'React Client',
    summary: 'React 19 + TypeScript UI — 12 pages, 6 Zustand stores, React Query hooks.',
    color: '#0a6e53',
    tint: 'rgba(10, 110, 83, 0.07)',
    order: 1,
  },
  {
    id: 'ipc',
    label: 'IPC Bridge',
    summary: 'Tauri command surface (142 commands) with validation, write-gate and events.',
    color: '#1d4ed8',
    tint: 'rgba(29, 78, 216, 0.07)',
    order: 2,
  },
  {
    id: 'ingestion',
    label: 'Ingestion',
    summary: 'Gmail OAuth, historical scan, live polling, Gate 1-2-3 message gating.',
    color: '#0369a1',
    tint: 'rgba(3, 105, 161, 0.07)',
    order: 3,
  },
  {
    id: 'extraction',
    label: 'Extraction & AI',
    summary: 'Six-layer extraction ladder, normalization, fingerprinting, rule learning.',
    color: '#7c3aed',
    tint: 'rgba(124, 58, 237, 0.07)',
    order: 4,
  },
  {
    id: 'statements',
    label: 'Statements',
    summary: 'PDF password resolution, parsing, Instrument Gate, draft stage/commit.',
    color: '#c2410c',
    tint: 'rgba(194, 65, 12, 0.07)',
    order: 5,
  },
  {
    id: 'queues',
    label: 'Queues',
    summary: 'Four bounded ingestion queues converging on reconciliation.',
    color: '#0e7490',
    tint: 'rgba(14, 116, 144, 0.07)',
    order: 6,
  },
  {
    id: 'reconciliation',
    label: 'Reconciliation',
    summary: 'Prefilter → exact → scored → cluster → canonical ledger write.',
    color: '#15803d',
    tint: 'rgba(21, 128, 61, 0.07)',
    order: 7,
  },
  {
    id: 'licensing',
    label: 'Licensing',
    summary: 'License state machine, JWT verification, device binding, write-gate.',
    color: '#be123c',
    tint: 'rgba(190, 18, 60, 0.07)',
    order: 8,
  },
  {
    id: 'persistence',
    label: 'Persistence',
    summary: 'SQLCipher SQLite (43 tables), macOS Keychain secrets, encrypted backups.',
    color: '#475569',
    tint: 'rgba(71, 85, 105, 0.08)',
    order: 9,
  },
  {
    id: 'workers',
    label: 'Background Workers',
    summary: 'Boot-spawned loops: polling, maintenance, health, updates, incident response.',
    color: '#a16207',
    tint: 'rgba(161, 98, 7, 0.07)',
    order: 10,
  },
  {
    id: 'sidecars',
    label: 'Sidecar Processes',
    summary: 'Isolated OS processes: pdf_sidecar (pdfium) and llama_sidecar (llama-server).',
    color: '#4338ca',
    tint: 'rgba(67, 56, 202, 0.07)',
    order: 11,
  },
  {
    id: 'external',
    label: 'External Services',
    summary: 'Gmail API, Google OAuth, licensing backend (Vercel + Neon + Razorpay), model CDNs.',
    color: '#57534e',
    tint: 'rgba(87, 83, 78, 0.06)',
    order: 12,
  },
];

export const LAYER_BY_ID: Record<string, LayerDef> = Object.fromEntries(
  LAYERS.map((l) => [l.id, l])
);
