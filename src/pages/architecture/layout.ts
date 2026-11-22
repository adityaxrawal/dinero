/**
 * Layered-band layout for the system map.
 *
 * Follows the classic layered-architecture diagram structure: full-width
 * grouping containers stacked vertically in request-lifecycle order (external
 * at the top, persistence at the foundation), with components packed
 * left-to-right inside each band. Bands are real React Flow nodes so they pan
 * and zoom with the graph — not screen-space backdrops.
 */
import type { Edge, Node, ReactFlowInstance } from '@xyflow/react';
import type { ArchFlowNode, BandFlowNode } from './nodes';
import {
  NODE_W,
  NODE_H,
  BAND_PAD_TOP,
  BAND_PAD_X,
  BAND_GAP,
  BAND_V_GAP,
  NODE_V_GAP,
  MAX_BAND_W,
  BAND_ORDER as SHARED_BAND_ORDER,
} from './data';
import type { ComponentDef, ConnectionDef, LayerId } from './data';
import { MODEL, LAYERS, LAYER_DISPLAY_BY_ID } from './data';

export const BAND_ORDER = SHARED_BAND_ORDER as LayerId[];

const LAYER_BY_ID = Object.fromEntries(LAYERS.map((l) => [l.id, l]));

function nodeWidth(): number {
  return NODE_W;
}
function nodeHeight(): number {
  return NODE_H;
}

export interface BandBox {
  id: LayerId;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Computes absolute positions for band containers and their components. */
export function computeSystemLayout(hiddenLayers: Set<LayerId> = new Set()): {
  nodes: (ArchFlowNode | BandFlowNode)[];
  boxes: BandBox[];
} {
  const nodes: (ArchFlowNode | BandFlowNode)[] = [];
  const boxes: BandBox[] = [];
  let cursorY = 0;

  for (const layerId of BAND_ORDER) {
    if (hiddenLayers.has(layerId)) continue;
    const layer = LAYER_BY_ID[layerId];
    const members = MODEL.components.filter((c) => c.layer === layerId);
    if (members.length === 0 || !layer) continue;

    // Pack components into rows without exceeding the band width.
    const rows: ComponentDef[][] = [[]];
    let rowWidth = 0;
    for (const c of members) {
      const cw = nodeWidth();
      const projected = rowWidth + (rows[rows.length - 1].length > 0 ? BAND_GAP : 0) + cw;
      if (projected > MAX_BAND_W && rows[rows.length - 1].length > 0) {
        rows.push([]);
        rowWidth = 0;
      }
      const added = rowWidth + (rows[rows.length - 1].length > 0 ? BAND_GAP : 0) + cw;
      rows[rows.length - 1].push(c);
      rowWidth = added;
    }

    // Row heights accommodate the tallest node in the row (diamonds are taller).
    const rowHeights = rows.map((row) => Math.max(...row.map(() => nodeHeight())));
    const contentH = rowHeights.reduce((a, b) => a + b, 0) + (rows.length - 1) * NODE_V_GAP;
    const bandH = BAND_PAD_TOP + contentH + BAND_PAD_X;

    const maxRowW = Math.max(
      ...rows.map((row) => {
        let w = 0;
        row.forEach((c, i) => {
          w += (i > 0 ? BAND_GAP : 0) + nodeWidth();
        });
        return w;
      })
    );
    const bandW = maxRowW + BAND_PAD_X * 2;

    boxes.push({ id: layerId, x: 0, y: cursorY, width: bandW, height: bandH });
    const display = LAYER_DISPLAY_BY_ID[layerId];
    nodes.push({
      id: `band-${layerId}`,
      type: 'band',
      position: { x: 0, y: cursorY },
      width: bandW,
      height: bandH,
      data: {
        code: display?.code ?? '?',
        name: display?.name ?? layer.label,
        tech: display?.tech ?? layer.summary,
        color: display?.color ?? layer.color,
        count: members.length,
        trust: layerId === 'external' ? 'external' : 'internal',
      },
      zIndex: -1,
      selectable: false,
      draggable: false,
    } as BandFlowNode);
    // The title is its own node above the connector layer (zIndex 2) on an
    // opaque plate, so vertical runs pass *under* it instead of through it.
    // pointer-events: none lets clicks fall through to the band.
    nodes.push({
      id: `bandlabel-${layerId}`,
      type: 'bandLabel',
      position: { x: BAND_PAD_X - 8, y: cursorY + 6 },
      data: nodes[nodes.length - 1].data,
      zIndex: 2,
      selectable: false,
      draggable: false,
      focusable: false,
      style: { pointerEvents: 'none' },
    } as unknown as BandFlowNode);

    let rowY = cursorY + BAND_PAD_TOP;
    rows.forEach((row, r) => {
      let x = BAND_PAD_X;
      const rowH = rowHeights[r];
      row.forEach((c) => {
        const cw = nodeWidth();
        const ch = nodeHeight();
        nodes.push({
          id: c.id,
          type: 'arch',
          position: { x, y: rowY + (rowH - ch) / 2 },
          // Explicit size: the packer and the card must agree on the box,
          // otherwise content-sized cards overlap their row neighbours.
          width: cw,
          height: ch,
          data: { component: c },
          zIndex: 1,
        } as ArchFlowNode);
        x += cw + BAND_GAP;
      });
      rowY += rowH + NODE_V_GAP;
    });

    cursorY += bandH + BAND_V_GAP;
  }

  // One width for every band: a single aligned grid with a clean right edge,
  // and narrow layers no longer truncate their titles.
  const fullW = Math.max(...boxes.map((b) => b.width));
  for (const b of boxes) b.width = fullW;
  for (const n of nodes) if (n.type === 'band') n.width = fullW;

  return { nodes, boxes };
}

/** A precomputed connector path plus where its label sits (flow coords). */
export interface EdgeRoute {
  path: string;
  labelX: number;
  labelY: number;
}

type Rect = { x: number; y: number; w: number; h: number };

/** Label positions along a run, every 5%, nearest the middle first. */
const RUN_SAMPLES = Array.from({ length: 19 }, (_, i) => (i + 1) / 20).sort(
  (a, b) => Math.abs(a - 0.5) - Math.abs(b - 0.5)
);

/** Orthogonal polyline with small rounded corners. */
function roundedPath(pts: [number, number][], r = 5): string {
  const p = pts.filter((q, i) => i === 0 || q[0] !== pts[i - 1][0] || q[1] !== pts[i - 1][1]);
  let d = `M ${p[0][0]} ${p[0][1]}`;
  for (let i = 1; i < p.length - 1; i++) {
    const [ax, ay] = p[i - 1];
    const [bx, by] = p[i];
    const [cx, cy] = p[i + 1];
    const r1 = Math.min(r, Math.hypot(bx - ax, by - ay) / 2, Math.hypot(cx - bx, cy - by) / 2);
    const ux = Math.sign(bx - ax);
    const uy = Math.sign(by - ay);
    const vx = Math.sign(cx - bx);
    const vy = Math.sign(cy - by);
    d += ` L ${bx - ux * r1} ${by - uy * r1} Q ${bx} ${by} ${bx + vx * r1} ${by + vy * r1}`;
  }
  const last = p[p.length - 1];
  return `${d} L ${last[0]} ${last[1]}`;
}

/**
 * Gutter router for the band layout.
 *
 * Every band shares one column grid, so the 14px gaps between columns (and the
 * band paddings) are card-free at every height, as are the gaps between rows
 * and between bands. A connector leaves its source's right edge into the
 * column gutter beside it, travels vertically there, crosses horizontally in
 * the free gap just above (or below) the target's row, then drops down the
 * target's left gutter into its left edge. No segment can cross a card.
 * Parallel segments sharing a gutter or gap are spread into lanes, and each
 * label is placed on a horizontal run (which lies in a gap) avoiding cards and
 * earlier labels.
 */
export function routeSystemEdges(
  connections: ConnectionDef[],
  nodes: (ArchFlowNode | BandFlowNode)[],
  boxes: BandBox[]
): Map<string, EdgeRoute> {
  const rect = new Map<string, Rect>();
  for (const n of nodes) {
    if (n.type === 'arch')
      rect.set(n.id, { x: n.position.x, y: n.position.y, w: NODE_W, h: NODE_H });
  }
  const live = connections.filter((c) => rect.has(c.from) && rect.has(c.to));

  // Free horizontal corridors just below / above a card's row.
  const rowTops = [...new Set([...rect.values()].map((r) => r.y))].sort((a, b) => a - b);
  const bandOf = (y: number) => boxes.find((b) => y >= b.y && y < b.y + b.height);
  const gapBelow = (r: Rect) => {
    const band = bandOf(r.y);
    const nextRow = rowTops.find((t) => t > r.y && band && t < band.y + band.height);
    return nextRow !== undefined
      ? r.y + r.h + NODE_V_GAP / 2
      : r.y + r.h + BAND_PAD_X + BAND_V_GAP / 2;
  };
  const gapAbove = (r: Rect) => {
    const band = bandOf(r.y);
    const prevRow = [...rowTops].reverse().find((t) => t < r.y && band && t >= band.y);
    return prevRow !== undefined ? r.y - NODE_V_GAP / 2 : r.y - BAND_PAD_TOP - BAND_V_GAP / 2;
  };
  const gutterRight = (r: Rect) => r.x + r.w + BAND_GAP / 2;
  const gutterLeft = (r: Rect) => r.x - BAND_GAP / 2;

  // Lane bookkeeping: members per shared corridor, so overlapping runs fan out.
  const groups = new Map<string, string[]>();
  const join = (key: string, id: string) => {
    const g = groups.get(key) ?? [];
    if (!g.includes(id)) g.push(id);
    groups.set(key, g);
  };
  const plan = live.map((c) => {
    const a = rect.get(c.from)!;
    const b = rect.get(c.to)!;
    const gx1 = gutterRight(a);
    const gx2 = gutterLeft(b);
    const sameRow = a.y === b.y;
    const down = b.y > a.y;
    const gy = sameRow || !down ? gapBelow(sameRow ? a : b) : gapAbove(b);
    const straight = sameRow && Math.abs(gx1 - gx2) < 1; // adjacent cards
    const oneGutter = !sameRow && Math.abs(gx1 - gx2) < 1; // target is the next column
    join(`out:${c.from}`, c.id);
    join(`in:${c.to}`, c.id);
    if (!straight) {
      join(`v:${gx1}`, c.id);
      if (!oneGutter) {
        join(`h:${gy}`, c.id);
        join(`v:${gx2}`, c.id);
      }
    }
    return { c, a, b, gx1, gx2, gy, straight, oneGutter };
  });
  const lane = (key: string, id: string, step: number, max: number) => {
    const g = groups.get(key) ?? [id];
    const off = (g.indexOf(id) - (g.length - 1) / 2) * step;
    return Math.max(-max, Math.min(max, off));
  };

  // Obstacles for labels: cards plus each band's title plate (estimated from
  // its text — 12.5px uppercase with wide tracking ≈ 9.5px per character).
  const cards: Rect[] = [
    ...rect.values(),
    ...boxes.map((b) => {
      const d = LAYER_DISPLAY_BY_ID[b.id];
      const chars = (d?.code.length ?? 1) + (d?.name.length ?? 0);
      return { x: BAND_PAD_X - 8, y: b.y + 6, w: chars * 9.5 + 30, h: 34 };
    }),
  ];
  const placed: Rect[] = [];
  const bandGapYs = boxes.map((b) => b.y + b.height + BAND_V_GAP / 2);

  const out = new Map<string, EdgeRoute>();
  // Place the most constrained labels first: short edges have only the gaps
  // next to their endpoints, long ones can fall back to gaps along the way.
  const byConstraint = [...plan].sort((p, q) => Math.abs(p.b.y - p.a.y) - Math.abs(q.b.y - q.a.y));
  for (const { c, a, b, gx1, gx2, gy, straight, oneGutter } of byConstraint) {
    const sy = a.y + a.h / 2 + lane(`out:${c.from}`, c.id, 9, 36);
    const ty = b.y + b.h / 2 + lane(`in:${c.to}`, c.id, 9, 36);
    let pts: [number, number][];
    let seg: [[number, number], [number, number]] | null = null; // label run
    if (straight) {
      pts = [
        [a.x + a.w, sy],
        [b.x, sy],
      ];
    } else {
      const x1 = gx1 + lane(`v:${gx1}`, c.id, 2.5, 5.5);
      if (oneGutter) {
        pts = [
          [a.x + a.w, sy],
          [x1, sy],
          [x1, ty],
          [b.x, ty],
        ];
      } else {
        const inBandGap = bandGapYs.some((y) => Math.abs(y - gy) < 1);
        const y = gy + lane(`h:${gy}`, c.id, inBandGap ? 3 : 2.5, inBandGap ? 13 : 5.5);
        const x2 = gx2 + lane(`v:${gx2}`, c.id, 2.5, 5.5);
        pts = [
          [a.x + a.w, sy],
          [x1, sy],
          [x1, y],
          [x2, y],
          [x2, ty],
          [b.x, ty],
        ];
        seg = [
          [x1, y],
          [x2, y],
        ];
      }
    }

    // Label: on the horizontal run if there is one, otherwise in a band gap
    // the vertical run crosses (between bands, clear of cards).
    const lw = c.label.length * 5.6 + 12;
    const lh = 14;
    const candidates: [number, number][] = [];
    if (seg) {
      // Band gaps are 34px tall: a label may also ride just above/below its run.
      const dys = bandGapYs.some((y) => Math.abs(y - gy) < 1) ? [0, -8, 8] : [0];
      for (const dy of dys) {
        for (const t of RUN_SAMPLES) {
          // centre on the gap, not the lane-shifted run, so the 14px label
          // never pokes into the cards bounding a 16px row gap
          candidates.push([seg[0][0] + (seg[1][0] - seg[0][0]) * t, gy + dy]);
        }
      }
      // Many edges converge on one corridor (e.g. every DB write into SQLite's
      // band gap). When that corridor is full, fall back to where this edge's
      // own vertical run crosses an earlier band gap — free space, and it
      // spreads the labels along their routes.
      const x1 = pts[1][0];
      const lo = Math.min(sy, gy);
      const hi = Math.max(sy, gy);
      for (const y of bandGapYs.filter((g) => g > lo + 1 && g < hi - 1)) {
        for (const dy of [0, -8, 8]) {
          for (const dx of [0, -(lw / 2 + 4), lw / 2 + 4]) candidates.push([x1 + dx, y + dy]);
        }
      }
    } else if (straight) {
      // Adjacent cards leave only a 14px gutter: float the label in the free
      // row gap just below (or above) the short connector instead.
      const mx = (a.x + a.w + b.x) / 2;
      for (const dx of [0, -60, 60, -120, 120, -180, 180, -240, 240]) {
        candidates.push([mx + dx, gapBelow(a)], [mx + dx, gapAbove(a)]);
      }
    } else {
      const lo = Math.min(sy, ty);
      const hi = Math.max(sy, ty);
      const x = pts[1][0];
      const ys = [gy, ...bandGapYs.filter((g) => g > lo && g < hi)];
      for (const dx of [0, -60, 60]) for (const y of ys) candidates.push([x + dx, y]);
    }
    candidates.push([
      (pts[0][0] + pts[pts.length - 1][0]) / 2,
      (pts[0][1] + pts[pts.length - 1][1]) / 2,
    ]);
    // First collision-free spot wins; if none is free, take the least-bad one
    // (overlapping a card costs more than overlapping another label).
    const overlap = (r: Rect, list: Rect[]) =>
      list.reduce(
        (sum, o) =>
          sum +
          Math.max(0, Math.min(r.x + r.w, o.x + o.w) - Math.max(r.x, o.x)) *
            Math.max(0, Math.min(r.y + r.h, o.y + o.h) - Math.max(r.y, o.y)),
        0
      );
    let at = candidates[0];
    let best = Number.POSITIVE_INFINITY;
    for (const cand of candidates) {
      const r = { x: cand[0] - lw / 2, y: cand[1] - lh / 2, w: lw, h: lh };
      const cost = overlap(r, cards) * 2 + overlap(r, placed);
      if (cost < best) {
        best = cost;
        at = cand;
        if (cost === 0) break;
      }
    }
    placed.push({ x: at[0] - lw / 2, y: at[1] - lh / 2, w: lw, h: lh });
    out.set(c.id, { path: roundedPath(pts), labelX: at[0], labelY: at[1] });
  }
  return out;
}

/** Connector objects for the system map, each carrying its routed path. */
export function buildSystemEdges(
  connections: ConnectionDef[],
  nodes: (ArchFlowNode | BandFlowNode)[],
  boxes: BandBox[]
) {
  const routes = routeSystemEdges(connections, nodes, boxes);
  return connections.map((c) => ({
    id: c.id,
    source: c.from,
    target: c.to,
    type: 'arch' as const,
    data: { kind: c.kind, edgeLabel: c.label, route: routes.get(c.id) },
  }));
}

/** BFS over the connection graph to collect the neighbourhood of a component. */
export function neighbourhood(
  rootId: string,
  depth: number,
  direction: 'both' | 'upstream' | 'downstream'
): { nodes: Set<string>; edges: Set<string> } {
  const nodes = new Set<string>([rootId]);
  const edges = new Set<string>();
  let frontier = [rootId];

  for (let d = 0; d < depth; d++) {
    const next: string[] = [];
    for (const id of frontier) {
      for (const c of MODEL.connections) {
        if (direction !== 'downstream' && c.to === id && !nodes.has(c.from)) {
          nodes.add(c.from);
          edges.add(c.id);
          next.push(c.from);
        }
        if (direction !== 'upstream' && c.from === id && !nodes.has(c.to)) {
          nodes.add(c.to);
          edges.add(c.id);
          next.push(c.to);
        }
      }
    }
    frontier = next;
    if (frontier.length === 0) break;
  }
  for (const c of MODEL.connections) {
    if (nodes.has(c.from) && nodes.has(c.to)) edges.add(c.id);
  }
  return { nodes, edges };
}

/**
 * Components matching a free-text query (name, layer, purpose, technologies,
 * sources). Name matches come first so the top results are the obvious ones.
 */
export function searchComponents(query: string): Set<string> {
  const q = query.trim().toLowerCase();
  if (!q) return new Set();
  const byName: string[] = [];
  const rest: string[] = [];
  for (const c of MODEL.components) {
    const layer = LAYER_DISPLAY_BY_ID[c.layer];
    const haystack = [
      c.name,
      layer?.name ?? '',
      layer?.tech ?? '',
      c.purpose,
      c.description,
      ...c.technologies,
      ...c.sources,
      ...c.responsibilities,
    ]
      .join(' ')
      .toLowerCase();
    if (c.name.toLowerCase().includes(q)) byName.push(c.id);
    else if (haystack.includes(q)) rest.push(c.id);
  }
  return new Set([...byName, ...rest]);
}

/**
 * Fit with a readable zoom floor. If the graph is still wider than the canvas,
 * align its left edge (where these left-to-right diagrams start) instead of
 * centring it, which would hide the entry point off-screen.
 */
export async function fitFromStart<N extends Node, E extends Edge>(
  rf: ReactFlowInstance<N, E>,
  canvasWidth: number,
  padding: number
) {
  await rf.fitView({ padding, minZoom: 0.7 });
  const vp = rf.getViewport();
  const b = rf.getNodesBounds(rf.getNodes());
  if (b.width * vp.zoom > canvasWidth) await rf.setViewport({ ...vp, x: 24 - b.x * vp.zoom });
}
