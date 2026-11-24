/**
 * Type definitions for the interactive architecture model.
 *
 * The model is plain data — components, connections, workflows, schema and
 * state machines — describing the code as built (the authority for behaviour;
 * design intent and decisions live in dinero-docs and its ADR log). Counts are
 * in ./counts.ts and checked against the code by model.test.ts.
 * Every view (system map, data flow, workflows, ER, state machines) renders
 * from these structures, so the diagrams cannot drift from one another.
 */

/** Coarse architectural layer, used for grouping and colour coding. */
export type LayerId =
  | 'client'
  | 'ipc'
  | 'ingestion'
  | 'extraction'
  | 'statements'
  | 'queues'
  | 'reconciliation'
  | 'licensing'
  | 'persistence'
  | 'workers'
  | 'sidecars'
  | 'external';

/** What kind of thing a component is. Drives node shape and legend. */
export type ComponentKind =
  | 'ui' // React screens / panels
  | 'service' // long-lived Rust module or worker
  | 'command' // IPC command surface
  | 'gate' // decision / validation point
  | 'queue' // bounded async queue
  | 'store' // database / keychain / file store
  | 'process' // isolated sidecar process
  | 'external' // third-party service
  | 'worker'; // background loop

/** How two components talk to each other. */
export type RelationKind =
  | 'sync' // request/response (IPC invoke, function call)
  | 'async' // fire-and-forget task / spawned job
  | 'event' // Tauri event emission
  | 'db' // SQLite read/write
  | 'network' // outbound HTTP
  | 'secret' // Keychain access
  | 'pipe' // stdin/stdout to sidecar process
  | 'gate'; // license write-gate check

/** Evidence level for a claim in the model. */
export type Confidence = 'verified' | 'inferred' | 'gap';

/** A layer's visual definition. */
export interface LayerDef {
  id: LayerId;
  label: string;
  /** Short description shown in the legend. */
  summary: string;
  /** Tailwind-compatible accent colour for nodes in this layer. */
  color: string;
  /** Background tint for the layer group box. */
  tint: string;
  /** Sort order on the system map. */
  order: number;
}

/** One architectural component. */
export interface ComponentDef {
  id: string;
  name: string;
  layer: LayerId;
  kind: ComponentKind;
  /** One-line purpose, shown on the node. */
  purpose: string;
  /** Longer description for the detail panel. */
  description: string;
  /** Concrete responsibilities (bullet list in the panel). */
  responsibilities: string[];
  /** Frameworks / libraries / protocols used. */
  technologies: string[];
  /** Data consumed. */
  inputs: string[];
  /** Data produced. */
  outputs: string[];
  /** Source paths in the real codebase (may be empty for external systems). */
  sources: string[];
  /** Named IPC commands, events or tables this component owns. */
  apiSurface: string[];
  /** Key operational facts (worker counts, capacities, cadences). */
  facts: string[];
  /** Evidence level for this entry. */
  confidence: Confidence;
  /** Optional notes on known gaps / discrepancies found during verification. */
  notes?: string[];
}

/** A directed relationship between two components. */
export interface ConnectionDef {
  id: string;
  from: string;
  to: string;
  kind: RelationKind;
  /** Short label shown on the edge (e.g. "invoke", "transaction job"). */
  label: string;
  /** Optional longer explanation for the detail panel. */
  detail?: string;
}

/** One step in an end-to-end workflow. */
export interface WorkflowStep {
  id: string;
  /** Short title. */
  title: string;
  /** What happens in this step. */
  detail: string;
  /** Components that participate in this step. */
  components: string[];
  /** Relation kinds exercised in this step (for highlighting). */
  relations: RelationKind[];
  /** Optional decision outcome labels (e.g. "pass / reject / defer"). */
  outcomes?: { label: string; detail: string }[];
}

/** An end-to-end workflow (sequence of steps). */
export interface WorkflowDef {
  id: string;
  name: string;
  summary: string;
  /** Participant lanes, left to right (component ids). */
  participants: string[];
  steps: WorkflowStep[];
}

/** A database entity (table). */
export interface EntityDef {
  id: string;
  name: string;
  /** Group tag for colouring (identity / ledger / statements / recon / ops). */
  group: 'identity' | 'ledger' | 'statements' | 'reconciliation' | 'ops';
  columns: { name: string; type: string; key?: 'PK' | 'FK' | 'UQ' | 'V'; note?: string }[];
  description: string;
}

/** A relationship between two entities. */
export interface EntityRelation {
  from: string;
  to: string;
  cardinality: string; // e.g. "||--o{"
  label: string;
  /** Not enforced by a FOREIGN KEY (soft reference or lifecycle link). */
  logical?: boolean;
}

/** A state in a state machine. */
export interface StateDef {
  id: string;
  label: string;
  /** Terminal for the machine's local transitions. */
  terminal?: boolean;
  /** Entry pseudo-state. */
  start?: boolean;
  note?: string;
}

export interface StateTransition {
  from: string;
  to: string;
  label: string;
  /** Explains who/what causes the transition. */
  detail?: string;
  /**
   * Set when the move comes from a verified server answer rather than a local
   * rule (license only). Drawn as a network edge; excluded from the
   * is_legal_transition check.
   */
  server?: true;
}

export interface StateMachineDef {
  id: string;
  name: string;
  summary: string;
  /** Source of truth in code. */
  source: string;
  states: StateDef[];
  transitions: StateTransition[];
  /** Verification note (e.g. enforced vs aspirational). */
  notes: string[];
}

/** Everything a view needs to render. */
export interface ArchitectureModel {
  layers: LayerDef[];
  components: ComponentDef[];
  connections: ConnectionDef[];
  workflows: WorkflowDef[];
  entities: EntityDef[];
  entityRelations: EntityRelation[];
  stateMachines: StateMachineDef[];
}
