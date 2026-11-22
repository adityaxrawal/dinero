/**
 * Assembles the complete architecture model from its parts.
 * Every view consumes this single object, so diagrams cannot drift apart.
 */
import type { ArchitectureModel } from './types';
import { LAYERS } from './layers';
import { COMPONENTS } from './components';
import { CONNECTIONS } from './connections';
import { WORKFLOWS } from './workflows';
import { ENTITIES, ENTITY_RELATIONS, SCHEMA_FACTS } from './database';
import { STATE_MACHINES } from './stateMachines';
import { setNodeCodeOrder } from './presentation';

// Unique `N1…N47` card codes — the old two-letter prefix collided for 11 groups.
setNodeCodeOrder(COMPONENTS.map((c) => c.id));

export const MODEL: ArchitectureModel = {
  layers: LAYERS,
  components: COMPONENTS,
  connections: CONNECTIONS,
  workflows: WORKFLOWS,
  entities: ENTITIES,
  entityRelations: ENTITY_RELATIONS,
  stateMachines: STATE_MACHINES,
};

export { SCHEMA_FACTS };

/** Component id → definition lookup. */
export const COMPONENT_BY_ID: Record<string, (typeof COMPONENTS)[number]> = Object.fromEntries(
  COMPONENTS.map((c) => [c.id, c])
);

/** Ids of every component referenced by a connection but missing from the inventory. */
export function danglingComponentIds(): string[] {
  const known = new Set(COMPONENTS.map((c) => c.id));
  const missing = new Set<string>();
  for (const c of CONNECTIONS) {
    if (!known.has(c.from)) missing.add(c.from);
    if (!known.has(c.to)) missing.add(c.to);
  }
  for (const w of WORKFLOWS) {
    for (const s of w.steps) for (const id of s.components) if (!known.has(id)) missing.add(id);
  }
  return [...missing];
}

export * from './types';
export { LAYERS, LAYER_BY_ID } from './layers';
export { COMPONENTS } from './components';
export { CONNECTIONS, connectionsFor } from './connections';
export { WORKFLOWS, WORKFLOW_BY_ID } from './workflows';
export { ENTITIES, ENTITY_RELATIONS } from './database';
export { STATE_MACHINES } from './stateMachines';
export {
  STAGES,
  PATHS,
  EDGES,
  DF_COLORS,
  type Stage,
  type StageId,
  type FlowEdge,
} from './dataflow';
export {
  THEME,
  FONTS,
  LAYER_DISPLAY,
  LAYER_DISPLAY_BY_ID,
  EDGE_KIND_DISPLAY,
  NODE_KIND_LABEL,
  INVARIANTS,
  STAT_DEFS,
  nodeCode,
  nodeKindLabel,
  type LayerDisplay,
  NODE_W,
  NODE_H,
  BAND_PAD_TOP,
  BAND_PAD_X,
  BAND_GAP,
  BAND_V_GAP,
  NODE_V_GAP,
  MAX_BAND_W,
  BAND_ORDER,
} from './presentation';
