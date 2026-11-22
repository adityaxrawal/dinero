/**
 * Workflow explorer — UML sequence-diagram notation.
 *
 * Lifelines (dashed verticals) hang off participant boxes; each step renders
 * as a labelled message between lifelines with activation bars, and decision
 * outcomes appear as dashed `alt` fragments. Messages are derived from the
 * shared connection model so the sequence cannot disagree with the system map.
 */
import { useMemo } from 'react';
import { GitBranch } from 'lucide-react';
import {
  MODEL,
  COMPONENT_BY_ID,
  type WorkflowDef,
  type WorkflowStep,
  type ConnectionDef,
  type RelationKind,
  nodeKindLabel,
} from '../data';
import { EDGE_STYLE } from '../nodes';

const COL_W = 178;
const HEADER_H = 52;
const MSG_H = 34;
const STEP_GAP = 26;
const PAD_X = 24;

interface Message {
  from: string;
  to: string;
  label: string;
  kind: RelationKind;
}

/** Derives the messages of a step from the connection model. */
function stepMessages(step: WorkflowStep, participantSet: Set<string>): Message[] {
  const inStep = new Set(step.components);
  const out: Message[] = [];
  const seen = new Set<string>();
  for (const c of MODEL.connections as ConnectionDef[]) {
    if (!inStep.has(c.from) || !inStep.has(c.to) || c.from === c.to) continue;
    if (!participantSet.has(c.from) || !participantSet.has(c.to)) continue;
    const key = `${c.from}>${c.to}:${c.label}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ from: c.from, to: c.to, label: c.label, kind: c.kind });
  }
  return out;
}

interface Props {
  workflowId: string;
  /** Owned by the explorer so the footer player and this diagram agree. */
  activeStep: number | null;
  onStep: (index: number) => void;
  onSelectComponent: (id: string) => void;
}

export function WorkflowView({ workflowId, activeStep, onStep, onSelectComponent }: Props) {
  const workflow: WorkflowDef = useMemo(
    () => MODEL.workflows.find((w) => w.id === workflowId) ?? MODEL.workflows[0],
    [workflowId]
  );

  const participants = workflow.participants;
  const participantSet = useMemo(() => new Set(participants), [participants]);
  const colIndex = useMemo(() => {
    const m = new Map<string, number>();
    participants.forEach((p, i) => m.set(p, i));
    return m;
  }, [participants]);

  /** Vertical layout of every step's messages. */
  const layout = useMemo(() => {
    // Outcomes render as 2-line-clamped rows; budget accordingly so the
    // fragment can never run into the next step's click strip.
    const rows = workflow.steps.map((s) => {
      const msgs = stepMessages(s, participantSet);
      const height =
        30 +
        Math.max(msgs.length, 1) * MSG_H +
        (s.outcomes?.length ? 36 + s.outcomes.length * 28 : 0);
      return { step: s, msgs, height };
    });
    const out: { step: WorkflowStep; msgs: Message[]; y: number; height: number }[] = [];
    let y = HEADER_H + 18;
    for (const r of rows) {
      out.push({ ...r, y });
      y += r.height + STEP_GAP;
    }
    return out;
  }, [workflow, participantSet]);

  const totalH = layout.length
    ? layout[layout.length - 1].y + layout[layout.length - 1].height + 40
    : 400;
  const totalW = PAD_X * 2 + participants.length * COL_W;
  const active = activeStep !== null ? (workflow.steps[activeStep] ?? null) : null;

  const cx = (id: string) => PAD_X + (colIndex.get(id) ?? 0) * COL_W + COL_W / 2;

  return (
    <div className="flex h-full">
      {/* Sequence diagram */}
      <div className="relative flex-1 overflow-auto bg-[#FDF7EC]">
        <div className="relative" style={{ width: totalW, height: totalH, minWidth: '100%' }}>
          {/* Lifelines */}
          {participants.map((p) => (
            <div
              key={`life-${p}`}
              className="absolute border-l border-dashed"
              style={{
                left: cx(p),
                top: HEADER_H,
                height: totalH - HEADER_H,
                borderColor: active ? 'rgba(11,74,58,0.28)' : 'rgba(11,74,58,0.22)',
              }}
              aria-hidden
            />
          ))}

          {/* Participant headers */}
          {participants.map((p, i) => {
            const comp = COMPONENT_BY_ID[p];
            const involved = active?.components.includes(p);
            return (
              <button
                key={p}
                onClick={() => onSelectComponent(p)}
                className="absolute rounded-md border-[1.6px] bg-white px-2 py-1.5 text-center shadow-sm transition-all hover:shadow-md"
                style={{
                  left: PAD_X + i * COL_W + 12,
                  top: 8,
                  width: COL_W - 24,
                  borderColor: involved ? '#0B4A3A' : '#0B4A3A55',
                  boxShadow: involved ? '0 0 0 2px rgba(11,74,58,0.18)' : undefined,
                }}
              >
                <div className="text-[8px] font-semibold uppercase tracking-[0.1em] text-[#0B4A3A]/80">
                  {comp ? nodeKindLabel(comp.kind) : 'component'}
                </div>
                <div className="mt-0.5 truncate text-[10px] font-bold text-[#12312B]">
                  {comp?.name ?? p}
                </div>
              </button>
            );
          })}

          {/* Steps */}
          {layout.map(({ step, msgs, y, height }, si) => {
            const isActive = si === activeStep;
            return (
              <div
                key={step.id}
                className="absolute left-0 cursor-pointer"
                style={{
                  top: y,
                  width: totalW,
                  height,
                  background: isActive ? 'rgba(11,74,58,0.055)' : 'transparent',
                  borderLeft: isActive ? '3px solid #0B4A3A' : '3px solid transparent',
                }}
                onClick={() => onStep(si)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    onStep(si);
                  }
                }}
                aria-expanded={isActive}
                aria-label={`Step ${si + 1}: ${step.title}`}
              >
                {/* Step number + title */}
                <div className="flex items-center gap-2" style={{ paddingLeft: 12, paddingTop: 4 }}>
                  <span
                    className="flex h-[18px] w-[18px] items-center justify-center rounded-full text-[9px] font-bold"
                    style={{
                      background: isActive ? '#0B4A3A' : 'rgba(11,74,58,0.12)',
                      color: isActive ? 'white' : '#0B4A3A',
                    }}
                  >
                    {si + 1}
                  </span>
                  <span className="text-[10.5px] font-semibold text-[#12312B]">{step.title}</span>
                  <span className="flex gap-1">
                    {step.relations.map((r) => (
                      <span
                        key={r}
                        className="rounded-sm bg-[#0B4A3A]/8 px-1 py-px text-[7.5px] font-semibold uppercase tracking-wide text-[#0B4A3A]/80"
                      >
                        {r}
                      </span>
                    ))}
                  </span>
                </div>

                {/* Messages */}
                <svg
                  className="pointer-events-none absolute inset-0"
                  width={totalW}
                  height={height}
                  aria-hidden
                >
                  {/* A step with no cross-lane message happens inside its
                      component(s): show activation bars instead of a blank row. */}
                  {msgs.length === 0 && (
                    <>
                      {step.components.map((cid) => (
                        <rect
                          key={`act-${cid}`}
                          x={cx(cid) - 3}
                          y={26}
                          width={6}
                          height={26}
                          rx="1.5"
                          fill="#0B4A3A"
                          opacity={isActive ? 0.3 : 0.18}
                        />
                      ))}
                      <text
                        x={cx(step.components[0]) + 10}
                        y={42}
                        fontSize="8"
                        fontWeight="600"
                        fill="#0B4A3ACC"
                      >
                        internal processing
                      </text>
                    </>
                  )}
                  {msgs.map((m, mi) => {
                    const x1 = cx(m.from);
                    const x2 = cx(m.to);
                    const my = 30 + mi * MSG_H + 8;
                    const dir = x2 > x1 ? 1 : -1;
                    const headX = x2 - dir * 2;
                    return (
                      <g key={`${m.from}-${m.to}-${mi}`}>
                        {/* activation bar on receiver */}
                        <rect
                          x={x2 - 3}
                          y={my - 12}
                          width={6}
                          height={26}
                          rx="1.5"
                          fill="#0B4A3A"
                          opacity={isActive ? 0.22 : 0.12}
                        />
                        <line
                          x1={x1 + dir * 4}
                          y1={my}
                          x2={x2 - dir * 8}
                          y2={my}
                          stroke={EDGE_STYLE[m.kind].stroke}
                          strokeWidth="1.4"
                          strokeDasharray={EDGE_STYLE[m.kind].dash || undefined}
                          markerEnd="none"
                        />
                        <polygon
                          points={`${headX},${my} ${headX - dir * 8},${my - 4} ${headX - dir * 8},${my + 4}`}
                          fill={EDGE_STYLE[m.kind].stroke}
                        />
                        <g
                          transform={`translate(${(x1 + x2) / 2 - (Math.min(m.label.length * 6.2 + 12, COL_W - 12)) / 2}, ${my - 18})`}
                        >
                          <rect
                            width={Math.min(m.label.length * 6.2 + 12, COL_W - 12)}
                            height={14}
                            rx="3"
                            fill="white"
                            stroke="rgba(11,74,58,0.22)"
                            strokeWidth="0.8"
                          />
                          <text
                            x={Math.min(m.label.length * 6.2 + 12, COL_W - 12) / 2}
                            y={10}
                            textAnchor="middle"
                            fontSize="8"
                            fontWeight="600"
                            fill="#0B4A3ACC"
                          >
                            {m.label.length > 26 ? `${m.label.slice(0, 25)}…` : m.label}
                          </text>
                        </g>
                      </g>
                    );
                  })}
                </svg>

                {/* Messages as DOM (clickable labels) */}
                {msgs.map((m, mi) => (
                  <div
                    key={`hit-${m.from}-${m.to}-${mi}`}
                    className="absolute cursor-default"
                    style={{
                      left: Math.min(cx(m.from), cx(m.to)),
                      top: 30 + mi * MSG_H - 6,
                      width: Math.abs(cx(m.to) - cx(m.from)),
                      height: 22,
                    }}
                    title={`${COMPONENT_BY_ID[m.from]?.name ?? m.from} → ${COMPONENT_BY_ID[m.to]?.name ?? m.to}: ${m.label}`}
                  />
                ))}

                {/* alt fragment: decision outcomes */}
                {step.outcomes && step.outcomes.length > 0 && (
                  <div
                    className="absolute rounded border border-dashed border-[#C2410C]/55 bg-[#C2410C]/[0.045] px-2.5 py-1.5"
                    style={{
                      left: 14,
                      top: 30 + Math.max(msgs.length, 1) * MSG_H + 2,
                      width: Math.min(totalW - 40, 620),
                    }}
                  >
                    <div className="flex items-center gap-1 text-[8px] font-bold uppercase tracking-[0.11em] text-[#C2410C]">
                      <GitBranch size={9} /> alt — decision outcomes
                    </div>
                    <ul className="mt-1 space-y-0.5">
                      {step.outcomes.map((o) => (
                        <li
                          key={o.label}
                          className="text-[9px] leading-snug text-[#12312B]/78 line-clamp-2"
                          title={o.detail}
                        >
                          <span className="font-semibold text-[#12312B]">[{o.label}]</span>{' '}
                          {o.detail}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
