/**
 * Workflow player — the explorer's footer bar.
 *
 * Mirrors the reference explorer: title + description, prev / play / next /
 * close controls, and a horizontally scrolling strip of step cards. Selecting
 * a step highlights the participating components on the graph and centres it.
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import type { WorkflowDef } from './data';
import { MODEL, COMPONENT_BY_ID, EDGE_KIND_DISPLAY } from './data';

interface Props {
  workflowId: string | null;
  stepIndex: number;
  onStep: (i: number) => void;
  onClose: () => void;
}

export function WorkflowPlayer({ workflowId, stepIndex, onStep, onClose }: Props) {
  const workflow: WorkflowDef | null = useMemo(
    () => (workflowId ? (MODEL.workflows.find((w) => w.id === workflowId) ?? null) : null),
    [workflowId]
  );
  const [playing, setPlaying] = useState(false);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  // The interval restarts whenever the step advances so it always schedules
  // `current + 1` — a single long-lived closure would capture a stale index
  // and replay one step forever.
  useEffect(() => {
    if (timer.current) {
      clearInterval(timer.current);
      timer.current = null;
    }
    if (playing && workflow) {
      timer.current = setInterval(() => {
        onStep(stepIndex + 1);
      }, 2200);
    }
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing, workflow?.id, stepIndex]);

  // Stop when the run finishes.
  useEffect(() => {
    if (workflow && stepIndex >= workflow.steps.length - 1 && playing) setPlaying(false);
  }, [stepIndex, workflow, playing]);

  if (!workflow) return null;
  const step = workflow.steps[Math.min(stepIndex, workflow.steps.length - 1)];

  return (
    <footer className="archx-wfbar" aria-label="Workflow player">
      <div className="archx-wf-top">
        <div>
          <div className="t">{workflow.name}</div>
        </div>
        <div className="d">{workflow.summary}</div>
        <div className="archx-wf-nav" style={{ display: 'flex', gap: 6, marginLeft: 'auto' }}>
          <button
            className="archx-tbtn"
            data-play
            onClick={() => setPlaying(!playing)}
            aria-label={playing ? 'Pause' : 'Play'}
          >
            {playing ? '❚❚ Pause' : '▶ Play'}
          </button>
          <button
            className="archx-tbtn"
            data-prev
            onClick={() => onStep(stepIndex - 1)}
            aria-label="Previous step"
          >
            ← Prev
          </button>
          <button
            className="archx-tbtn"
            data-next
            onClick={() => onStep(stepIndex + 1)}
            aria-label="Next step"
          >
            Next →
          </button>
          <button
            className="archx-tbtn"
            data-close
            onClick={() => {
              setPlaying(false);
              onClose();
            }}
            aria-label="Close workflow"
          >
            ✕ Close
          </button>
        </div>
      </div>
      <div className="archx-wf-steps">
        {workflow.steps.map((s, i) => (
          <button
            key={s.id}
            className={'archx-step' + (i === stepIndex ? ' on' : '')}
            data-step={i}
            onClick={() => {
              setPlaying(false);
              onStep(i);
            }}
            aria-current={i === stepIndex ? 'step' : undefined}
          >
            <div className="n">
              {String(i + 1).padStart(2, '0')} ·{' '}
              <span style={{ color: EDGE_KIND_DISPLAY[s.relations[0] ?? '']?.color ?? undefined }}>
                {s.relations[0] ?? 'step'}
              </span>
            </div>
            <div className="t">{s.title}</div>
            <div
              className="k"
              title={s.components.map((c) => COMPONENT_BY_ID[c]?.name ?? c).join(' · ')}
            >
              {s.components
                .slice(0, 3)
                .map((c) => COMPONENT_BY_ID[c]?.name ?? c)
                .join(' · ')}
              {s.components.length > 3 ? ` +${s.components.length - 3}` : ''}
            </div>
          </button>
        ))}
      </div>
      {step && (
        <div
          style={{
            padding: '0 16px 12px',
            color: 'var(--ax-text-2)',
            fontSize: 10.5,
            lineHeight: 1.6,
            maxWidth: 980,
          }}
        >
          <b style={{ color: 'var(--ax-text)' }}>{step.title}</b> — {step.detail}
          {step.outcomes && step.outcomes.length > 0 && (
            <div style={{ marginTop: 5, display: 'flex', flexWrap: 'wrap', gap: 6 }}>
              {step.outcomes.map((o) => (
                <span key={o.label} className="archx-chip accent" title={o.detail}>
                  alt [{o.label}]
                </span>
              ))}
            </div>
          )}
        </div>
      )}
    </footer>
  );
}
