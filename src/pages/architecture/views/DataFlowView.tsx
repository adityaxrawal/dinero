/**
 * Dynamic data-flow view — the master source → ledger → UI pipeline, rendered
 * as a directed graph where picking a stage highlights its full path and shows
 * which components implement it. Diagram data lives in `../data/dataflow`.
 */
import { useMemo, useRef, useState } from 'react';
import { ReactFlow, Background, Panel, type Edge, type Node } from '@xyflow/react';
import { nodeTypes, edgeTypes, makeEdgeMarker } from '../nodes';
import { fitFromStart } from '../layout';
import { COMPONENT_BY_ID, STAGES, PATHS, EDGES, DF_COLORS as C, type StageId } from '../data';

interface Props {
  selectedStage: StageId | null;
  onSelectStage: (id: StageId | null) => void;
}

function DataFlowInner({ selectedStage, onSelectStage }: Props) {
  const [activePath, setActivePath] = useState<string | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);

  const pathStages = useMemo(() => {
    const set = new Set<StageId>();
    if (activePath) {
      const p = PATHS.find((x) => x.id === activePath);
      if (p) p.stages.forEach((s) => set.add(s));
    }
    return set;
  }, [activePath]);

  const nodes: Node[] = useMemo(
    () =>
      STAGES.map((s) => {
        const inPath = pathStages.size === 0 || pathStages.has(s.id);
        return {
          id: s.id,
          type: 'stage',
          position: { x: s.x, y: s.y },
          data: {
            label: s.label,
            sublabel: s.sub,
            color: s.color,
            shape: s.shape,
            dimmed: !inPath,
            matched: selectedStage === s.id,
            selected: selectedStage === s.id,
            onSelect: () => onSelectStage(selectedStage === s.id ? null : (s.id as StageId)),
          },
        };
      }),
    [pathStages, selectedStage, onSelectStage]
  );

  const edges: Edge[] = useMemo(
    () =>
      EDGES.map((e, i) => {
        const inPath = pathStages.size === 0 || (pathStages.has(e.from) && pathStages.has(e.to));
        return {
          id: `fe-${i}`,
          source: e.from,
          target: e.to,
          type: 'arch',
          markerEnd: makeEdgeMarker('sync'),
          animated: inPath && pathStages.size > 0,
          data: {
            kind: 'sync' as const,
            edgeLabel: e.label,
            labelsOn: true,
            // flows into a parked / discarded stage are not the happy path
            dashed: STAGES.find((st) => st.id === e.to)?.color === C.drop,
            dimmed: !inPath,
            highlighted: inPath && pathStages.size > 0,
          },
        };
      }),
    [pathStages]
  );

  return (
    <div className="flex h-full">
      <div ref={canvasRef} className="relative flex-1">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onInit={(rf) => void fitFromStart(rf, canvasRef.current?.clientWidth ?? 0, 0.15)}
          minZoom={0.15}
          maxZoom={1.8}
          proOptions={{ hideAttribution: true }}
          nodesDraggable={false}
          nodesConnectable={false}
          panOnScroll
          aria-label="Data flow from source to ledger"
        >
          <Background color="rgba(6,78,59,0.10)" gap={22} size={1} />
          <Panel position="top-left">
            <div className="w-[260px] rounded-lg border border-emerald-ink/15 bg-white/90 p-3 backdrop-blur">
              <div className="text-[10px] font-semibold uppercase tracking-[0.08em] text-emerald-ink/80">
                Trace a path
              </div>
              <p className="mt-1 text-[10px] leading-snug text-emerald-ink/80">
                Pick an end-to-end flow to highlight it, or click any stage to see the components
                that implement it.
              </p>
              <div className="mt-2 space-y-1">
                {PATHS.map((p) => (
                  <button
                    key={p.id}
                    onClick={() => setActivePath(activePath === p.id ? null : p.id)}
                    className={
                      'block w-full rounded px-2 py-1 text-left text-[10px] transition-colors ' +
                      (activePath === p.id
                        ? 'bg-emerald-ink text-white'
                        : 'bg-emerald-ink/6 text-emerald-ink hover:bg-emerald-ink/12')
                    }
                  >
                    {p.name}
                  </button>
                ))}
              </div>
              <div className="mt-2 flex flex-wrap gap-x-2 gap-y-0.5 text-[8.5px] text-emerald-ink/80">
                <span>
                  <span
                    className="mr-0.5 inline-block h-2 w-2 rounded-sm align-middle"
                    style={{ background: C.gate }}
                  />{' '}
                  gate
                </span>
                <span>
                  <span
                    className="mr-0.5 inline-block h-2 w-2 rounded-sm align-middle"
                    style={{ background: C.flow }}
                  />{' '}
                  flow
                </span>
                <span>
                  <span
                    className="mr-0.5 inline-block h-2 w-2 rounded-sm align-middle"
                    style={{ background: C.ai }}
                  />{' '}
                  LLM
                </span>
                <span>
                  <span
                    className="mr-0.5 inline-block h-2 w-2 rounded-sm align-middle"
                    style={{ background: C.drop }}
                  />{' '}
                  parked / discarded
                </span>
                <span>
                  <span
                    className="mr-0.5 inline-block h-2 w-2 rounded-sm align-middle"
                    style={{ background: C.good }}
                  />{' '}
                  ledger
                </span>
              </div>
            </div>
          </Panel>
        </ReactFlow>
      </div>
    </div>
  );
}

export function DataFlowView(props: Props) {
  return <DataFlowInner {...props} />;
}

/** Stage detail for the inspector (rendered by the explorer, not the canvas). */
export function StageDetail({
  selectedStage,
  onSelectComponent,
}: {
  selectedStage: StageId | null;
  onSelectComponent: (id: string) => void;
}) {
  const stage = STAGES.find((s) => s.id === selectedStage) ?? null;
  return (
    <div className="px-4 py-3">
      {stage ? (
        <>
          <div className="text-[10px] font-semibold uppercase tracking-[0.08em] text-emerald-ink/80">
            Pipeline stage
          </div>
          <h2 className="mt-0.5 text-[13px] font-semibold text-emerald-ink">{stage.label}</h2>
          <p className="mt-2 text-[11px] leading-relaxed text-emerald-ink/80">{stage.detail}</p>
          <h3 className="mt-4 text-[10px] font-semibold uppercase tracking-[0.08em] text-emerald-ink/80">
            Implemented by
          </h3>
          <ul className="mt-1 space-y-1">
            {stage.components.map((cid) => {
              const comp = COMPONENT_BY_ID[cid];
              return comp ? (
                <li key={cid}>
                  <button
                    onClick={() => onSelectComponent(cid)}
                    className="w-full rounded bg-white/70 px-2 py-1.5 text-left text-[10.5px] text-emerald-ink hover:bg-white"
                  >
                    {comp.name}
                    <div className="text-[9px] text-emerald-ink/80">{comp.purpose}</div>
                  </button>
                </li>
              ) : null;
            })}
          </ul>
        </>
      ) : (
        <>
          <h2 className="text-[13px] font-semibold text-emerald-ink">Data flow</h2>
          <p className="mt-1.5 text-[11px] leading-relaxed text-emerald-ink/80">
            Source → gates / parsing → queues → reconciliation → ledger → UI. Every entry point
            (historical scan, live polling, manual upload) converges on the same processing path —
            there is no weaker-validated second path.
          </p>
          <p className="mt-2 text-[11px] leading-relaxed text-emerald-ink/80">
            Click a stage to inspect it; use the path list to trace a complete request lifecycle
            including its parked and discarded branches.
          </p>
        </>
      )}
    </div>
  );
}
