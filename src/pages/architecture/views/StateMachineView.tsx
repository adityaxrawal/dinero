/**
 * State machines — UML state-diagram notation: filled disc for the initial
 * pseudo-state, rounded rectangles for states, bullseye for the final state,
 * and labelled transition arrows between them.
 */
import { useCallback, useMemo, useRef, useState } from 'react';
import { ReactFlow, Background, Panel, type Edge, type Node } from '@xyflow/react';
import {
  nodeTypes,
  edgeTypes,
  makeEdgeMarker,
  type StateFlowNode,
  type StateNodeData,
} from '../nodes';
import { fitFromStart } from '../layout';
import { MODEL, type StateDef, type StateMachineDef } from '../data';

const STATE_COLOR = '#0369A1';
const TERMINAL_COLOR = '#BE123C';
const START_COLOR = '#12312B';

/**
 * Layered left-to-right placement: a state's column is its BFS distance from
 * the initial pseudo-state, so forward transitions always point right and the
 * layout follows the machine's real topology.
 */
function layoutStates(sm: StateMachineDef): Node[] {
  const level = new Map<string, number>();
  const start = sm.states.find((s) => s.start);
  const queue = start ? [start.id] : [];
  if (start) level.set(start.id, 0);
  while (queue.length > 0) {
    const id = queue.shift()!;
    for (const t of sm.transitions) {
      if (t.from === id && !level.has(t.to)) {
        level.set(t.to, level.get(id)! + 1);
        queue.push(t.to);
      }
    }
  }
  const columns = new Map<number, StateDef[]>();
  for (const s of sm.states) {
    const l = level.get(s.id) ?? 0;
    columns.set(l, [...(columns.get(l) ?? []), s]);
  }
  // Column pitch = state card + room for the longest transition label (the
  // same width estimate ArchEdge uses for its label pill).
  const labelW = Math.max(...sm.transitions.map((t) => t.label.length * 5.6 + 28));
  const colW = 170 + labelW;
  const nodes: Node[] = [];
  for (const [l, states] of columns) {
    states.forEach((s, i) => {
      const kind = s.start ? 'start' : s.terminal ? 'end' : 'state';
      nodes.push({
        id: `${sm.id}:${s.id}`,
        type: 'state',
        position: { x: l * colW, y: (i - (states.length - 1) / 2) * 130 },
        data: {
          label: s.label,
          note: s.note,
          kind,
          color: kind === 'start' ? START_COLOR : kind === 'end' ? TERMINAL_COLOR : STATE_COLOR,
        },
      });
    });
  }
  return nodes;
}

interface Props {
  machineId: string;
  onSelectMachine: (id: string) => void;
}

function StateMachineInner({ machineId, onSelectMachine }: Props) {
  const canvasRef = useRef<HTMLDivElement>(null);
  const sm = useMemo(
    () => MODEL.stateMachines.find((m) => m.id === machineId) ?? MODEL.stateMachines[0],
    [machineId]
  );
  const [selectedState, setSelectedState] = useState<string | null>(null);

  const nodes = useMemo(() => layoutStates(sm), [sm]);

  const edges: Edge[] = useMemo(() => {
    const x = new Map(nodes.map((n) => [n.id, n.position.x]));
    return sm.transitions.map((t, i) => {
      const source = `${sm.id}:${t.from}`;
      const target = `${sm.id}:${t.to}`;
      // A transition back to an earlier (or same) column loops over the top.
      const back = (x.get(target) ?? 0) <= (x.get(source) ?? 0);
      return {
        id: `${sm.id}-t-${i}`,
        source,
        target,
        sourceHandle: back ? 't-s' : 'r',
        targetHandle: back ? 't-t' : 'l',
        type: 'arch',
        markerEnd: makeEdgeMarker(t.server ? 'network' : 'sync'),
        data: {
          kind: t.server ? ('network' as const) : ('sync' as const),
          edgeLabel: t.label,
          labelsOn: true, // transition labels are the diagram's content
          highlighted: selectedState === t.from || selectedState === t.to,
          dimmed: selectedState !== null && selectedState !== t.from && selectedState !== t.to,
        },
      };
    });
  }, [sm, nodes, selectedState]);

  const decorated = useMemo(
    () =>
      nodes.map((n) => {
        const stateId = n.id.split(':')[1];
        const prev = n.data as StateNodeData;
        return {
          ...n,
          data: {
            ...prev,
            selected: selectedState === stateId,
            dimmed: selectedState !== null && selectedState !== stateId,
            onSelect: () => setSelectedState(selectedState === stateId ? null : stateId),
          },
        } as StateFlowNode;
      }),
    [nodes, selectedState]
  );

  const onNodeClick = useCallback(
    (_: unknown, n: { id: string }) => {
      const stateId = n.id.split(':')[1];
      setSelectedState(selectedState === stateId ? null : stateId);
    },
    [selectedState]
  );

  return (
    <div className="flex h-full">
      <aside className="w-[220px] flex-shrink-0 overflow-y-auto border-r border-[#064E3B]/15 bg-[#FBF3E4] px-3 py-3">
        <h2 className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[#064E3B]/80">
          State machines
        </h2>
        <ul className="mt-2 space-y-1">
          {MODEL.stateMachines.map((m) => (
            <li key={m.id}>
              <button
                onClick={() => onSelectMachine(m.id)}
                className={
                  'w-full rounded-md px-2.5 py-2 text-left transition-colors ' +
                  (m.id === sm.id
                    ? 'bg-[#064E3B] text-white shadow-sm'
                    : 'text-[#064E3B] hover:bg-[#064E3B]/8')
                }
              >
                <div className="text-[11px] font-semibold leading-tight">{m.name}</div>
                <div
                  className={
                    'mt-0.5 text-[9px] leading-snug ' +
                    (m.id === sm.id ? 'text-white/75' : 'text-[#064E3B]/80')
                  }
                >
                  {m.states.length} states · {m.transitions.length} transitions
                </div>
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-4 border-t border-[#064E3B]/12 pt-3 text-[9px] leading-relaxed text-[#064E3B]/80">
          <div className="flex items-center gap-1.5">
            <svg width="12" height="12" aria-hidden>
              <circle cx="6" cy="6" r="5" fill={START_COLOR} />
            </svg>
            initial
          </div>
          <div className="mt-1 flex items-center gap-1.5">
            <svg width="16" height="12" aria-hidden>
              <rect
                x="1"
                y="1"
                width="14"
                height="10"
                rx="3"
                fill="white"
                stroke={STATE_COLOR}
                strokeWidth="1.5"
              />
            </svg>
            state
          </div>
          <div className="mt-1 flex items-center gap-1.5">
            <svg width="14" height="12" aria-hidden>
              <circle cx="7" cy="6" r="5" fill="none" stroke={TERMINAL_COLOR} strokeWidth="1.6" />
              <circle cx="7" cy="6" r="2.6" fill={TERMINAL_COLOR} />
            </svg>
            final
          </div>
        </div>
      </aside>

      <div ref={canvasRef} className="relative flex-1">
        <ReactFlow
          key={sm.id} // remount per machine so it re-fits
          nodes={decorated}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodeClick={onNodeClick}
          onInit={(rf) => void fitFromStart(rf, canvasRef.current?.clientWidth ?? 0, 0.18)}
          minZoom={0.15}
          maxZoom={1.8}
          proOptions={{ hideAttribution: true }}
          nodesDraggable={false}
          nodesConnectable={false}
          panOnScroll
          aria-label={`${sm.name} state diagram`}
        >
          <Background color="rgba(11,74,58,0.09)" gap={22} size={1} />
          <Panel position="top-left">
            <div className="w-[290px] rounded-lg border border-[#064E3B]/15 bg-white/92 px-3 py-2.5 shadow-sm backdrop-blur">
              <div className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[#064E3B]/80">
                {sm.name}
              </div>
              <p className="mt-1 text-[10px] leading-snug text-[#064E3B]/80">{sm.summary}</p>
              <p className="mt-1.5 font-mono text-[8.5px] text-[#064E3B]/80">{sm.source}</p>
            </div>
          </Panel>
        </ReactFlow>
      </div>
    </div>
  );
}

export function StateMachineView(props: Props) {
  return <StateMachineInner {...props} />;
}

/** Notes + states for the inspector (rendered by the explorer, not the canvas). */
export function StateMachineDetail({ machineId }: { machineId: string }) {
  const sm = MODEL.stateMachines.find((m) => m.id === machineId) ?? MODEL.stateMachines[0];
  return (
    <div className="px-4 py-3">
      <h2 className="text-[12.5px] font-semibold text-[#064E3B]">Verification notes</h2>
      <ul className="mt-2 space-y-2">
        {sm.notes.map((n) => (
          <li
            key={n}
            className="rounded-md border border-amber-200/70 bg-amber-50/70 px-2.5 py-2 text-[10.5px] leading-relaxed text-amber-900"
          >
            {n}
          </li>
        ))}
      </ul>
      <h3 className="mt-4 text-[10px] font-semibold uppercase tracking-[0.08em] text-[#064E3B]/80">
        States
      </h3>
      <ul className="mt-1 space-y-1">
        {sm.states.map((s) => (
          <li
            key={s.id}
            className={
              'rounded px-2 py-1 text-[10.5px] ' +
              (s.terminal
                ? 'bg-rose-50 text-rose-900'
                : s.start
                  ? 'bg-[#064E3B]/8 text-[#064E3B]'
                  : 'bg-white/60 text-[#064E3B]/85')
            }
          >
            <span className="font-medium">{s.label}</span>
            {s.terminal && <span className="ml-1 text-[8.5px] uppercase">terminal</span>}
            {s.note && <div className="text-[9px] text-[#064E3B]/80">{s.note}</div>}
          </li>
        ))}
      </ul>
    </div>
  );
}
