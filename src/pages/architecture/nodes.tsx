/**
 * Explorer graph primitives — champagne/emerald node cards and cluster boxes.
 *
 * Node-card anatomy mirrors the reference explorer: `ID · kind` over the
 * component name with a role glyph, a one-line role, and up to three
 * technology chips, with the layer accent as a left bar. Cluster (layer) boxes
 * carry a code + name + tech + component-count header. Edges are orthogonal
 * connectors whose labels surface on hover or when labels are pinned on.
 */
import { memo } from 'react';
import {
  Handle,
  Position,
  BaseEdge,
  getSmoothStepPath,
  type EdgeProps,
  type Node,
  type NodeProps,
  MarkerType,
  useNodesStore,
} from '@xyflow/react';
import type { ComponentDef, RelationKind } from './data';
import { LAYER_DISPLAY_BY_ID, EDGE_KIND_DISPLAY, nodeCode, nodeKindLabel } from './data';
import { cn } from '@/lib/utils';

export { NODE_W, NODE_H } from './data';

/* ─────────────────────────── component node card ─────────────────────────── */

/**
 * Role glyph — the blueprint's shape language in miniature: a DB cylinder for
 * stores, stacked trays for queues, a cloud for externals, a shield for gates,
 * a browser frame for UI, a rack for workers, a gear for services.
 */
export function KindGlyph({ kind }: { kind: string }) {
  const common = {
    className: 'archx-kind-glyph',
    viewBox: '0 0 24 24',
    'aria-hidden': true as const,
  };
  switch (kind) {
    case 'store':
      return (
        <svg {...common}>
          <ellipse cx="12" cy="5.5" rx="8" ry="2.8" />
          <path d="M4 5.5v13c0 1.6 3.6 2.8 8 2.8s8-1.2 8-2.8v-13" />
          <path d="M4 12c0 1.6 3.6 2.8 8 2.8s8-1.2 8-2.8" />
        </svg>
      );
    case 'queue':
      return (
        <svg {...common}>
          <rect x="3" y="3.5" width="18" height="4.5" rx="1" />
          <rect x="3" y="9.8" width="18" height="4.5" rx="1" />
          <rect x="3" y="16" width="18" height="4.5" rx="1" />
        </svg>
      );
    case 'external':
      return (
        <svg {...common}>
          <path d="M7 19a4.5 4.5 0 0 1 .3-9 5.5 5.5 0 0 1 10.4-1.4A4.2 4.2 0 0 1 17.5 19z" />
        </svg>
      );
    case 'gate':
      return (
        <svg {...common}>
          <path d="M12 2.5l8 3v6.2c0 4.7-3.3 8.1-8 9.3-4.7-1.2-8-4.6-8-9.3V5.5z" />
          <path d="M8.8 12l2.3 2.3 4.1-4.4" />
        </svg>
      );
    case 'ui':
      return (
        <svg {...common}>
          <rect x="2.5" y="4" width="19" height="16" rx="2" />
          <path d="M2.5 9h19" />
          <circle cx="6" cy="6.5" r="0.6" />
        </svg>
      );
    case 'worker':
      return (
        <svg {...common}>
          <rect x="3" y="4" width="18" height="5" rx="1" />
          <rect x="3" y="11" width="18" height="5" rx="1" />
          <path d="M6.5 6.5h.01M6.5 13.5h.01M17 6.5h1.5M17 13.5h1.5" />
        </svg>
      );
    case 'command':
      return (
        <svg {...common}>
          <rect x="4.5" y="10" width="15" height="10" rx="2" />
          <path d="M8 10V7a4 4 0 0 1 8 0v3" />
        </svg>
      );
    case 'process':
      // isolated OS process: a box inside a box
      return (
        <svg {...common}>
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <rect x="8" y="8" width="8" height="8" rx="1" />
        </svg>
      );
    default: // service — a gear
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="3.2" />
          <path d="M12 2.8v2.6M12 18.6v2.6M2.8 12h2.6M18.6 12h2.6M5.5 5.5l1.8 1.8M16.7 16.7l1.8 1.8M18.5 5.5l-1.8 1.8M7.3 16.7l-1.8 1.8" />
        </svg>
      );
  }
}

export type ArchNodeData = Record<string, unknown> & {
  component: ComponentDef;
  dimmed?: boolean;
  matched?: boolean;
  focused?: boolean;
  selected?: boolean;
  onSelect?: (id: string) => void;
  onHover?: (id: string | null) => void;
};

export type ArchFlowNode = Node<ArchNodeData, 'arch'>;

export const ArchNode = memo(function ArchNode({ data, selected }: NodeProps<ArchFlowNode>) {
  const component = data.component;
  const layer = LAYER_DISPLAY_BY_ID[component.layer];
  const code = nodeCode(component.id);
  // Selection is app-driven via data.selected; the RF `selected` prop stays
  // false for controlled nodes, so prefer the data flag.
  const isSel = data.selected ?? selected;
  return (
    <div
      className={cn(
        'archx-node',
        `k-${component.kind}`, // per-kind outline (see architecture.css)
        data.dimmed && 'dim',
        isSel && 'sel',
        data.focused && 'hl',
        data.matched && 'matched'
      )}
      data-id={component.id}
      style={{
        ['--nc' as string]: layer.color,
      }}
      onClick={() => data.onSelect?.(component.id)}
      onMouseEnter={() => data.onHover?.(component.id)}
      onMouseLeave={() => data.onHover?.(null)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          data.onSelect?.(component.id);
        }
      }}
      aria-label={`${component.name} — ${component.purpose}`}
    >
      <div className="archx-node-id">
        <KindGlyph kind={component.kind} />
        {code} · {nodeKindLabel(component.kind)}
        {component.confidence !== 'verified' && component.confidence !== 'gap' && (
          <span style={{ color: 'var(--ax-warning-text)', marginLeft: 6 }}>
            {component.confidence}
          </span>
        )}
      </div>
      <div className="archx-node-name" title={component.name}>
        {component.name}
      </div>
      <div className="archx-node-role" title={component.purpose}>
        {component.purpose}
      </div>
      {component.technologies.length > 0 && (
        // One row only: chips ellipsize and extras collapse into "+n", so the
        // row can never push the name/role out of the fixed-height card.
        <div className="archx-node-tags" title={component.technologies.join(' · ')}>
          {component.technologies.slice(0, 2).map((t) => (
            <span key={t}>{t}</span>
          ))}
          {component.technologies.length > 2 && (
            <span className="more">+{component.technologies.length - 2}</span>
          )}
        </div>
      )}
      {component.confidence === 'gap' && <span className="gap-flag">gap</span>}
      <Handle type="target" position={Position.Left} className="!opacity-0" />
      <Handle type="source" position={Position.Right} className="!opacity-0" />
    </div>
  );
});

/* ─────────────────────────── cluster / layer box ─────────────────────────── */

export type BandNodeData = Record<string, unknown> & {
  code: string;
  name: string;
  tech: string;
  color: string;
  count: number;
  /** External/network-facing zones render as dashed trust boundaries. */
  trust?: 'internal' | 'external';
};

export type BandFlowNode = Node<BandNodeData, 'band'>;

export const BandNode = memo(function BandNode({ data }: NodeProps<BandFlowNode>) {
  return (
    <div
      className={cn('archx-band', data.trust === 'external' && 'untrusted')}
      style={{ ['--bc' as string]: data.color }}
      role="group"
      aria-label={`${data.name} layer`}
    >
      <div className="archx-band-count">{data.count} components</div>
    </div>
  );
});

/** Band title plate — rendered as a separate node above the edges. */
export const BandLabelNode = memo(function BandLabelNode({ data }: NodeProps<BandFlowNode>) {
  return (
    <div className="archx-band-label" style={{ ['--bc' as string]: data.color }} aria-hidden>
      <div className="archx-band-title">
        <span className="archx-band-code">{data.code}</span>
        <span className="archx-band-name">{data.name}</span>
      </div>
      <div className="archx-band-tech">{data.tech}</div>
    </div>
  );
});

/* ─────────────────────────── ER entity node ─────────────────────────── */

export type EntityNodeData = Record<string, unknown> & {
  name: string;
  groupLabel: string;
  color: string;
  columns: { name: string; type: string; key?: string; note?: string }[];
  dimmed?: boolean;
  selected?: boolean;
  onSelect?: (id: string) => void;
};

export type EntityFlowNode = Node<EntityNodeData, 'entity'>;

export const EntityNode = memo(function EntityNode({ data, selected }: NodeProps<EntityFlowNode>) {
  const isSel = data.selected ?? selected;
  return (
    <div
      className={cn('archx-node', isSel && 'sel', data.dimmed && 'dim')}
      style={{
        ['--nc' as string]: data.color,
        minWidth: 214,
        maxWidth: 238,
        height: 'auto',
        padding: 0,
        justifyContent: 'flex-start',
      }}
      onClick={() => data.onSelect?.(data.name)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          data.onSelect?.(data.name);
        }
      }}
      aria-label={`Table ${data.name}`}
    >
      <div className="archx-ent-hd">
        <span className="archx-ent-name" title={data.name}>
          {data.name}
        </span>
        <span className="archx-ent-group">{data.groupLabel}</span>
      </div>
      <div className="archx-ent-cols">
        {data.columns.map((col) => (
          <div key={col.name} className="archx-ent-row">
            <span
              className="archx-ent-key"
              style={{
                color:
                  col.key === 'PK'
                    ? '#0a6e53'
                    : col.key === 'FK'
                      ? 'var(--ax-warning-text)'
                      : col.key === 'UQ'
                        ? '#1d4ed8'
                        : 'var(--ax-text-3)',
              }}
            >
              {col.key ?? '·'}
            </span>
            <span className="archx-ent-col" title={col.name}>
              {col.name}
            </span>
            <span className="archx-ent-type" title={col.type}>
              {col.type}
            </span>
          </div>
        ))}
      </div>
      <Handle id="l" type="target" position={Position.Left} className="!opacity-0" />
      <Handle id="r" type="source" position={Position.Right} className="!opacity-0" />
      {/* same-lane relations loop on the right instead of wrapping the column */}
      <Handle id="r-t" type="target" position={Position.Right} className="!opacity-0" />
    </div>
  );
});

/* ─────────────────────────── UML state node ─────────────────────────── */

export type StateNodeData = Record<string, unknown> & {
  label: string;
  note?: string;
  kind: 'start' | 'end' | 'state';
  color: string;
  dimmed?: boolean;
  selected?: boolean;
  onSelect?: (id: string) => void;
};

export type StateFlowNode = Node<StateNodeData, 'state'>;

export const StateNode = memo(function StateNode({ data, selected }: NodeProps<StateFlowNode>) {
  const isSel = data.selected ?? selected;
  if (data.kind === 'start') {
    return (
      <div className="flex items-center" style={{ opacity: data.dimmed ? 0.32 : 1 }}>
        <svg width="20" height="20" aria-label="Initial state">
          <circle cx="10" cy="10" r="8" fill="var(--ax-ink)" />
        </svg>
        <span
          className="ml-2 text-[9px] font-semibold uppercase tracking-[0.14em]"
          style={{ color: 'var(--ax-text-2)' }}
        >
          {data.label}
        </span>
        <Handle id="r" type="source" position={Position.Right} className="!opacity-0" />
      </div>
    );
  }
  if (data.kind === 'end') {
    return (
      <div
        className="flex items-center cursor-pointer"
        style={{ opacity: data.dimmed ? 0.32 : 1 }}
        onClick={() => data.onSelect?.(data.label)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            data.onSelect?.(data.label);
          }
        }}
      >
        <Handle id="l" type="target" position={Position.Left} className="!opacity-0" />
        <Handle id="t-t" type="target" position={Position.Top} className="!opacity-0" />
        <svg width="24" height="24" aria-label="Final state">
          <circle cx="12" cy="12" r="10" fill="none" stroke="var(--ax-danger)" strokeWidth="2" />
          <circle cx="12" cy="12" r="5.5" fill="var(--ax-danger)" />
        </svg>
        <span
          className="ml-2 text-[9px] font-semibold uppercase tracking-[0.14em]"
          style={{ color: '#b91c1c' }} // danger red, darkened for legible small text
        >
          {data.label}
        </span>
      </div>
    );
  }
  return (
    <div
      className={cn('archx-node', isSel && 'sel', data.dimmed && 'dim')}
      style={{
        ['--nc' as string]: data.color,
        minWidth: 152,
        height: 'auto',
        alignItems: 'center',
        textAlign: 'center',
        borderRadius: 10,
      }}
      onClick={() => data.onSelect?.(data.label)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          data.onSelect?.(data.label);
        }
      }}
      aria-label={`State ${data.label}`}
    >
      <div className="archx-node-name" style={{ fontSize: 11.5 }}>
        {data.label}
      </div>
      {data.note && (
        <div className="archx-node-role" style={{ WebkitLineClamp: 2 }} title={data.note}>
          {data.note}
        </div>
      )}
      <Handle id="l" type="target" position={Position.Left} className="!opacity-0" />
      <Handle id="r" type="source" position={Position.Right} className="!opacity-0" />
      {/* back-edges loop over the top so they never share a forward path */}
      <Handle id="t-s" type="source" position={Position.Top} className="!opacity-0" />
      <Handle id="t-t" type="target" position={Position.Top} className="!opacity-0" />
    </div>
  );
});

/* ─────────────────────────── DFD stage node ─────────────────────────── */

export type StageNodeData = Record<string, unknown> & {
  label: string;
  sublabel?: string;
  color: string;
  shape?: 'process' | 'store' | 'entity' | 'terminator' | 'decision';
  dimmed?: boolean;
  matched?: boolean;
  selected?: boolean;
  onSelect?: (id: string) => void;
};

export type StageFlowNode = Node<StageNodeData, 'stage'>;

export const StageNode = memo(function StageNode({ data, selected }: NodeProps<StageFlowNode>) {
  const shape = data.shape ?? 'process';
  const isSel = data.selected ?? selected;
  return (
    <div
      // DFD notation per shape (stadium / open store / hexagon) lives in CSS
      className={cn(
        'archx-node',
        `s-${shape}`,
        isSel && 'sel',
        data.matched && 'matched',
        data.dimmed && 'dim'
      )}
      style={{
        ['--nc' as string]: data.color,
        ['--nb' as string]: data.color + '66',
        background: isSel ? 'var(--ax-card-hover)' : 'var(--ax-card)',
        minWidth: shape === 'decision' ? 188 : 168,
        height: 'auto',
        alignItems: 'center',
        textAlign: 'center',
      }}
      onClick={() => data.onSelect?.(String(data.label))}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          data.onSelect?.(String(data.label));
        }
      }}
    >
      <div
        className="archx-node-name"
        style={{ fontSize: 10.8, color: 'var(--ax-text)', whiteSpace: 'pre-line' }}
      >
        {data.label}
      </div>
      {data.sublabel && (
        <div
          className="archx-node-role"
          style={{ WebkitLineClamp: 2, color: 'var(--ax-text-2)' }}
          title={data.sublabel}
        >
          {data.sublabel}
        </div>
      )}
      <Handle type="target" position={Position.Left} className="!opacity-0" />
      <Handle type="source" position={Position.Right} className="!opacity-0" />
    </div>
  );
});

/* ─────────────────────────── edges ─────────────────────────── */

export const EDGE_STYLE: Record<RelationKind, { stroke: string; dash: string; width: number }> =
  Object.fromEntries(
    Object.entries(EDGE_KIND_DISPLAY).map(([k, v]) => [
      k,
      { stroke: v.color, dash: v.dash, width: k === 'gate' ? 1.6 : 1.5 },
    ])
  ) as Record<RelationKind, { stroke: string; dash: string; width: number }>;

export type CardEnd = 'one' | 'zero-one' | 'many' | 'zero-many';

function rotFor(pos: Position): number {
  switch (pos) {
    case Position.Left:
      return 0;
    case Position.Right:
      return 180;
    case Position.Top:
      return 90;
    default:
      return 270;
  }
}

function CardGlyph({
  at,
  pos,
  end,
  color,
}: {
  at: { x: number; y: number };
  pos: Position;
  end: CardEnd;
  color: string;
}) {
  return (
    <g
      transform={`translate(${at.x}, ${at.y}) rotate(${rotFor(pos)})`}
      stroke={color}
      strokeWidth="1.3"
      fill="none"
      strokeLinecap="round"
    >
      {end === 'one' && (
        <>
          <line x1="0" y1="-6" x2="0" y2="6" />
          <line x1="3.5" y1="-6" x2="3.5" y2="6" />
        </>
      )}
      {end === 'zero-one' && (
        <>
          <circle cx="4" cy="0" r="2.6" />
          <line x1="10" y1="-6" x2="10" y2="6" />
        </>
      )}
      {(end === 'many' || end === 'zero-many') && (
        <>
          {end === 'zero-many' && <circle cx="4" cy="0" r="2.6" />}
          <line x1={end === 'zero-many' ? 7 : 0} y1="0" x2={end === 'zero-many' ? 13 : 9} y2="-6" />
          <line x1={end === 'zero-many' ? 7 : 0} y1="0" x2={end === 'zero-many' ? 13 : 9} y2="0" />
          <line x1={end === 'zero-many' ? 7 : 0} y1="0" x2={end === 'zero-many' ? 13 : 9} y2="6" />
        </>
      )}
    </g>
  );
}

export type ArchEdgeData = Record<string, unknown> & {
  kind: RelationKind;
  edgeLabel: string;
  endpoints?: { source: CardEnd; target: CardEnd };
  /** Precomputed card-avoiding path (system map); otherwise smoothstep. */
  route?: { path: string; labelX: number; labelY: number } | undefined;
  /** Overrides the kind's dash: true = logical (dashed), false = solid. */
  dashed?: boolean | undefined;
  labelsOn?: boolean;
  dimmed?: boolean;
  highlighted?: boolean;
  onHover?: (on: boolean) => void;
};

export type ArchFlowEdge = EdgeProps & { data?: ArchEdgeData };

export const ArchEdge = memo(function ArchEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  selected,
  markerEnd,
}: ArchFlowEdge) {
  const [stepPath, stepX, stepY] = getSmoothStepPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    borderRadius: 13,
    offset: 18,
  });
  const route = data?.route;
  const path = route?.path ?? stepPath;
  const label = data?.edgeLabel ?? '';
  const nodeLookup = useNodesStore((s) => s.nodeLookup);
  // Routed labels (system map) were placed by the router. Smoothstep labels
  // sit at the path midpoint, which can land on a node: nudge to the nearest
  // spot that clears every measured node box.
  let labelX = route?.labelX ?? stepX;
  let labelY = route ? route.labelY : stepY - 7;
  if (!route && label) {
    const w = label.length * 5.6 + 12;
    const boxes: { x: number; y: number; w: number; h: number }[] = [];
    nodeLookup.forEach((n) => {
      const pos = n.internals.positionAbsolute;
      if (n.measured.width && n.measured.height)
        boxes.push({ x: pos.x, y: pos.y, w: n.measured.width, h: n.measured.height });
    });
    const clear = (x: number, y: number) =>
      !boxes.some(
        (b) => x - w / 2 < b.x + b.w && b.x < x + w / 2 && y - 8 < b.y + b.h && b.y < y + 6
      );
    // Near the anchor first, then fully beside it (half a label width + gap),
    // which clears loops that hug a node column (same-lane ER relations).
    const side = w / 2 + 6;
    const offsets = [
      [0, 0],
      [0, -14],
      [0, 14],
      [0, -28],
      [0, 28],
      [0, -42],
      [0, 42],
      [-50, 0],
      [50, 0],
      [-50, -14],
      [50, -14],
      [-50, 14],
      [50, 14],
      [0, -56],
      [0, 56],
      [side, 0],
      [-side, 0],
      [side, -14],
      [-side, -14],
      [side, 14],
      [-side, 14],
    ];
    const hit = offsets.find(([dx, dy]) => clear(labelX + dx, labelY + dy));
    if (hit) {
      labelX += hit[0];
      labelY += hit[1];
    }
  }
  const kind = (data?.kind ?? 'sync') as RelationKind;
  const style = EDGE_STYLE[kind];
  const highlighted = data?.highlighted || selected;
  const showLabel = (data?.labelsOn || highlighted) && !data?.dimmed;
  const er = data?.endpoints;

  return (
    <>
      <BaseEdge
        id={id}
        path={path}
        className={cn(highlighted && 'hl')}
        {...(er || !markerEnd ? {} : { markerEnd })}
        style={{
          stroke: style.stroke,
          strokeWidth: highlighted ? style.width + 0.9 : style.width,
          // unset → the kind's dash; true → logical (dashed); false → solid
          strokeDasharray:
            data?.dashed === undefined ? style.dash || undefined : data.dashed ? '4 3' : undefined,
          strokeOpacity: data?.dimmed ? 0.1 : highlighted ? 0.95 : 0.62,
          transition: 'opacity 180ms ease, stroke-width 180ms ease',
        }}
      />
      {er && !data?.dimmed && (
        <>
          <CardGlyph
            at={{ x: sourceX, y: sourceY }}
            pos={sourcePosition}
            end={er.source}
            color={style.stroke}
          />
          <CardGlyph
            at={{ x: targetX, y: targetY }}
            pos={targetPosition}
            end={er.target}
            color={style.stroke}
          />
        </>
      )}
      {label && !data?.dimmed && (
        <g
          transform={`translate(${labelX}, ${labelY})`}
          className={cn('archx-elabel pointer-events-none select-none', showLabel && 'show')}
        >
          <rect
            x={-(label.length * 2.8 + 6)}
            y={-8}
            width={label.length * 5.6 + 12}
            height={14}
            rx={3}
            fill="var(--ax-card)"
            stroke="var(--ax-border-subtle)"
            strokeWidth={0.8}
          />
          <text
            textAnchor="middle"
            y={2.5}
            style={{
              fontSize: 9.5,
              fill: highlighted ? 'var(--ax-text)' : 'var(--ax-text-2)',
              fontFamily: "'Inter', sans-serif",
              letterSpacing: '0.03em',
            }}
          >
            {label}
          </text>
        </g>
      )}
    </>
  );
});

/** Edge marker matching the reference explorer's flat arrowheads. */
export function makeEdgeMarker(kind: RelationKind) {
  return {
    type: MarkerType.ArrowClosed as const,
    color: EDGE_STYLE[kind].stroke,
    width: 14,
    height: 14,
  };
}

export const nodeTypes = {
  arch: ArchNode,
  band: BandNode,
  bandLabel: BandLabelNode,
  entity: EntityNode,
  state: StateNode,
  stage: StageNode,
};
export const edgeTypes = { arch: ArchEdge };
