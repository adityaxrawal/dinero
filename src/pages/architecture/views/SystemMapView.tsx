/**
 * System map — the full component inventory in layer clusters on a React Flow
 * canvas. The parent explorer owns selection, search and focus state; this view
 * renders it: node cards inside cluster boxes, orthogonal connectors whose
 * labels appear when pinned or highlighted, and a minimap.
 */
import { useCallback, useMemo, useEffect } from 'react';
import {
  ReactFlow,
  Background,
  MiniMap,
  useViewportStore,
  useReactFlow,
  type Edge,
} from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import type { LayerId } from '../data';
import { MODEL, LAYER_DISPLAY_BY_ID } from '../data';
import { buildSystemEdges, computeSystemLayout } from '../layout';
import { nodeTypes, edgeTypes, makeEdgeMarker, type ArchFlowNode, NODE_W, NODE_H } from '../nodes';

interface Props {
  hiddenLayers: Set<LayerId>;
  selectedId: string | null;
  searchHits: Set<string>;
  labelsOn: boolean;
  focus: { nodes: Set<string>; edges: Set<string> } | null;
  showMinimap: boolean;
  /** Rail click: frame this layer's band. `seq` makes repeat clicks re-frame. */
  layerFocus: { id: LayerId; seq: number } | null;
  onSelect: (id: string) => void;
  onClearSelection: () => void;
  onHover: (id: string | null) => void;
}

function SystemMapInner({
  hiddenLayers,
  selectedId,
  searchHits,
  labelsOn,
  focus,
  showMinimap,
  layerFocus,
  onSelect,
  onClearSelection,
  onHover,
}: Props) {
  const { setCenter, fitView, getZoom } = useReactFlow();
  const searching = searchHits.size > 0;
  // Level of detail: below 60% the card body is unreadable, so cards collapse
  // to a large name only. Boolean selector → re-renders only at the threshold.
  const far = useViewportStore((s) => s.transform[2] < 0.6);

  const base = useMemo(() => {
    const { nodes, boxes } = computeSystemLayout(hiddenLayers);
    return { nodes, edges: buildSystemEdges(MODEL.connections, nodes, boxes) };
  }, [hiddenLayers]);

  const nodes = useMemo(
    () =>
      base.nodes.map((n) => {
        if (n.type !== 'arch') return n;
        const id = n.id;
        const dimmed =
          (searching && !searchHits.has(id)) || (focus !== null && !focus.nodes.has(id));
        const matched = searching && searchHits.has(id);
        return {
          ...n,
          data: {
            ...n.data,
            dimmed,
            matched,
            focused: focus !== null && focus.nodes.has(id),
            selected: id === selectedId,
            onSelect,
            onHover,
          },
        } as ArchFlowNode;
      }),
    [base.nodes, searching, searchHits, focus, selectedId, onSelect, onHover]
  );

  const edges: Edge[] = useMemo(
    () =>
      base.edges.map((e) => {
        const kind = (e.data?.kind ?? 'sync') as Parameters<typeof makeEdgeMarker>[0];
        const inFocus = focus !== null && focus.edges.has(e.id);
        const highlighted =
          inFocus || (selectedId !== null && (e.source === selectedId || e.target === selectedId));
        const dimmed =
          (searching && !(searchHits.has(e.source) && searchHits.has(e.target))) ||
          (focus !== null && !focus.edges.has(e.id)) ||
          (selectedId !== null && !highlighted && focus === null);
        return {
          ...e,
          markerEnd: makeEdgeMarker(kind),
          data: { ...e.data, highlighted, dimmed, labelsOn },
        };
      }),
    [base.edges, selectedId, searching, searchHits, focus, labelsOn]
  );

  const onNodeClick = useCallback(
    (_: unknown, node: { id: string; type?: string | undefined }) => {
      // Band backgrounds are nodes too; clicking one counts as empty space.
      if (node.type === 'arch') onSelect(node.id);
      else onClearSelection();
    },
    [onSelect, onClearSelection]
  );

  useEffect(() => {
    if (!selectedId) return;
    const node = nodes.find((n) => n.id === selectedId);
    if (node && node.position) {
      setCenter(node.position.x + NODE_W / 2, node.position.y + NODE_H / 2, {
        zoom: Math.max(getZoom(), 0.92), // never zoom out on a user who zoomed in
        duration: 350,
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  // Re-frame whenever layers are hidden or restored, so the surviving graph
  // is never left half off-screen.
  useEffect(() => {
    fitView({ duration: 260, padding: 0.12 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hiddenLayers]);

  // Declared after the hiddenLayers effect so, on mount, the band framing wins.
  useEffect(() => {
    if (!layerFocus) return;
    fitView({ nodes: [{ id: `band-${layerFocus.id}` }], duration: 320, padding: 0.06 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layerFocus?.seq]);

  return (
    <div className={'relative h-full w-full' + (far ? ' archx-lod-far' : '')}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodeClick={onNodeClick}
        onPaneClick={onClearSelection}
        fitView
        fitViewOptions={{ padding: 0.1 }}
        minZoom={0.08}
        maxZoom={2}
        proOptions={{ hideAttribution: true }}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        panOnScroll
        zoomOnPinch
        aria-label="System architecture map"
      >
        <Background color="rgba(6, 78, 59, 0.10)" gap={40} size={1} />
        <Background color="rgba(6, 78, 59, 0.14)" gap={200} size={1} />
        {showMinimap && (
          <MiniMap
            pannable
            zoomable
            position="bottom-right"
            maskColor="rgba(232, 200, 136, 0.28)"
            nodeColor={(n) => {
              if (n.type === 'bandLabel') return 'transparent';
              if (n.type === 'band') return String(n.data?.color ?? 'var(--ax-text-3)') + '33';
              const layer = (n.data?.component as { layer?: string } | undefined)?.layer;
              return layer ? (LAYER_DISPLAY_BY_ID[layer]?.color ?? '#6b8a7f') : '#6b8a7f';
            }}
            style={{
              width: 132,
              height: 96,
              backgroundColor: 'var(--ax-card)',
              border: '1px solid var(--ax-border)',
            }}
          />
        )}
      </ReactFlow>
    </div>
  );
}

export function SystemMapView(props: Props) {
  return <SystemMapInner {...props} />;
}
