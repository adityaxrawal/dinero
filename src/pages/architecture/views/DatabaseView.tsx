/**
 * Database model — classic ER notation: entity boxes with a filled header and
 * attribute rows, connected by orthogonal lines carrying crow's-foot
 * cardinality glyphs (`||` exactly one, `o{` zero-or-many, …).
 */
import { useCallback, useMemo } from 'react';
import { ReactFlow, Background, Panel, type Edge, type Node } from '@xyflow/react';
import { nodeTypes, edgeTypes, type EntityFlowNode, type CardEnd, EDGE_STYLE } from '../nodes';
import { MODEL, type EntityDef } from '../data';

const GROUP_COLOR: Record<EntityDef['group'], string> = {
  identity: '#0A6E53',
  ledger: '#475569', // persistence layer
  statements: '#C2410C',
  reconciliation: '#15803D',
  ops: '#A16207', // background workers layer
};

const GROUP_LABEL: Record<EntityDef['group'], string> = {
  identity: 'Identity & consent',
  ledger: 'Ledger',
  statements: 'Statements',
  reconciliation: 'Reconciliation',
  ops: 'Operations',
};

/** Parses an ER cardinality string ("||--o{") into its two endpoint symbols. */
function parseEnds(card: string): { source: CardEnd; target: CardEnd } {
  const [src, tgt] = card.split('--');
  const map = (s: string): CardEnd => {
    if (s.includes('{')) return s.includes('o') ? 'zero-many' : 'many';
    if (s.includes('|')) return s.includes('o') ? 'zero-one' : 'one';
    return 'one';
  };
  return { source: map(src), target: map(tgt) };
}

/** Column-based layout: one lane per entity group. */
function layout(): Node[] {
  // Ordered so related groups sit side by side (ledger in the middle, between
  // statements and reconciliation; ops next to identity): only one relation
  // has to jump a lane, so links and labels stay in the gaps.
  const lanes: EntityDef['group'][] = ['identity', 'ops', 'statements', 'ledger', 'reconciliation'];
  const nodes: Node[] = [];
  const X_STEP = 380; // 142px between lanes leaves room for relation labels
  lanes.forEach((group, ci) => {
    const members = MODEL.entities.filter((e) => e.group === group);
    let y = 0;
    members.forEach((e) => {
      nodes.push({
        id: e.id,
        type: 'entity',
        position: { x: ci * X_STEP, y },
        data: {
          name: e.name,
          groupLabel: GROUP_LABEL[group],
          color: GROUP_COLOR[group],
          columns: e.columns,
        },
        zIndex: 1,
      });
      y += 74 + e.columns.length * 17;
    });
  });
  return nodes;
}

interface Props {
  selectedId: string | null;
  /** Toolbar "Labels": relationship verbs; otherwise shown on hover/selection. */
  labelsOn: boolean;
  onSelect: (id: string) => void;
}

function DatabaseInner({ selectedId, labelsOn, onSelect }: Props) {
  const nodes = useMemo(() => layout(), []);
  const laneX = useMemo(() => new Map(nodes.map((n) => [n.id, n.position.x])), [nodes]);

  const edges: Edge[] = useMemo(
    () =>
      MODEL.entityRelations.map((r, i) => ({
        id: `er-${i}`,
        source: r.from,
        target: r.to,
        sourceHandle: 'r',
        targetHandle: laneX.get(r.from) === laneX.get(r.to) ? 'r-t' : 'l',
        type: 'arch',
        data: {
          kind: 'db' as const,
          edgeLabel: r.label,
          endpoints: parseEnds(r.cardinality),
          dashed: r.logical ?? false, // enforced FKs solid, logical refs dashed
          labelsOn,
          highlighted: selectedId === r.from || selectedId === r.to,
          dimmed: selectedId !== null && selectedId !== r.from && selectedId !== r.to,
        },
      })),
    [selectedId, labelsOn, laneX]
  );

  const decorated = useMemo(
    () =>
      nodes.map((n) => ({
        ...n,
        data: {
          ...n.data,
          selected: n.id === selectedId,
          dimmed: selectedId !== null && n.id !== selectedId,
          onSelect,
        },
      })) as EntityFlowNode[],
    [nodes, selectedId, onSelect]
  );

  const onNodeClick = useCallback((_: unknown, n: { id: string }) => onSelect(n.id), [onSelect]);

  return (
    <div className="relative h-full w-full">
      <ReactFlow
        nodes={decorated}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodeClick={onNodeClick}
        fitView
        fitViewOptions={{ padding: 0.12 }}
        minZoom={0.12}
        maxZoom={1.8}
        proOptions={{ hideAttribution: true }}
        nodesDraggable={false}
        nodesConnectable={false}
        panOnScroll
        aria-label="Database entity relationship diagram"
      >
        <Background color="rgba(11,74,58,0.09)" gap={22} size={1} />
        <Panel position="top-left">
          <div className="rounded-lg border border-[#064E3B]/15 bg-white/90 px-3 py-2 shadow-sm backdrop-blur">
            <div className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[#064E3B]/80">
              Entity-relationship model
            </div>
            <div className="mt-0.5 text-[10px] text-[#064E3B]/80">
              43 tables · 60 migrations · 23 modelled here
            </div>
            <div className="mt-1.5 flex flex-wrap gap-x-2 gap-y-0.5 text-[8.5px] text-[#064E3B]/80">
              {Object.entries(GROUP_LABEL).map(([k, label]) => (
                <span key={k} className="inline-flex items-center gap-1">
                  <span
                    className="inline-block h-2 w-2 rounded-[2px] align-middle"
                    style={{ background: GROUP_COLOR[k as EntityDef['group']] }}
                  />
                  {label}
                </span>
              ))}
            </div>
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[8px] text-[#064E3B]/80">
              <svg width="34" height="12" aria-hidden>
                <line
                  x1="2"
                  y1="6"
                  x2="22"
                  y2="6"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.3"
                />
                <line
                  x1="4"
                  y1="1"
                  x2="4"
                  y2="11"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.3"
                />
                <line
                  x1="8"
                  y1="1"
                  x2="8"
                  y2="11"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.3"
                />
                <circle
                  cx="26"
                  cy="6"
                  r="2.4"
                  fill="none"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.2"
                />
                <line
                  x1="29"
                  y1="6"
                  x2="33"
                  y2="1.5"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.2"
                />
                <line
                  x1="29"
                  y1="6"
                  x2="33"
                  y2="6"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.2"
                />
                <line
                  x1="29"
                  y1="6"
                  x2="33"
                  y2="10.5"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.2"
                />
              </svg>
              exactly one → zero-or-many
              <svg width="26" height="12" aria-hidden>
                <line
                  x1="2"
                  y1="6"
                  x2="24"
                  y2="6"
                  stroke={EDGE_STYLE.db.stroke}
                  strokeWidth="1.3"
                  strokeDasharray="4 3"
                />
              </svg>
              logical reference (no FK constraint)
            </div>
          </div>
        </Panel>
      </ReactFlow>
    </div>
  );
}

export function DatabaseView(props: Props) {
  return <DatabaseInner {...props} />;
}
