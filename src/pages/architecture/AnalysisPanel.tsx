/**
 * Architecture Analysis pane — the explorer's default detail content.
 *
 * Covers the system's shape, its recurring patterns, the privacy invariants
 * that govern every design decision, the operational profile, and the known
 * gaps the verification surfaced. The text lives in the shared model
 * (`ANALYSIS`), so the standalone document renders the same words.
 */
import { Fragment } from 'react';
import { MODEL, INVARIANTS, SCHEMA_FACTS, ANALYSIS, analysisSegments } from './data';

function H({ n, children }: { n: string; children: React.ReactNode }) {
  return (
    <h3 className="archx-an-h">
      <span className="num" aria-hidden>
        {n}
      </span>
      {children}
    </h3>
  );
}

/** Renders shared analysis text (plain, **bold** and `code` segments). */
function Rich({ text }: { text: string }) {
  return (
    <>
      {analysisSegments(text).map((seg, i) =>
        seg.kind === 'b' ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: static text, never reordered
          <b key={i}>{seg.text}</b>
        ) : seg.kind === 'code' ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: static text, never reordered
          <code key={i}>{seg.text}</code>
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: static text, never reordered
          <span key={i}>{seg.text}</span>
        )
      )}
    </>
  );
}

export function AnalysisPanel() {
  const steps = MODEL.workflows.reduce((a, w) => a + w.steps.length, 0);
  return (
    <div className="archx-dp">
      <H n="01">System Shape</H>
      {ANALYSIS.shape.map((t) => (
        <p className="archx-an-p" key={t}>
          <Rich text={t} />
        </p>
      ))}

      <H n="02">Recurring Patterns</H>
      {ANALYSIS.patterns.map((t) => (
        <div className="archx-an-li" key={t}>
          <Rich text={t} />
        </div>
      ))}

      <H n="03">Privacy Invariants</H>
      {INVARIANTS.map((iv) => (
        <div className="archx-note" key={iv.title}>
          <b>{iv.title}</b>
          <div style={{ marginTop: 5 }}>{iv.body}</div>
        </div>
      ))}

      <H n="04">Operational Profile</H>
      <div className="archx-kv">
        {ANALYSIS.profile.map((row) => (
          <Fragment key={row.k}>
            <span className="k">{row.k}</span>
            <span className="v">{row.v}</span>
          </Fragment>
        ))}
      </div>

      <H n="05">Schema Facts</H>
      <table className="archx-table">
        <thead>
          <tr>
            <th>Fact</th>
            <th>Consequence</th>
          </tr>
        </thead>
        <tbody>
          {SCHEMA_FACTS.map((f) => (
            <tr key={f.fact}>
              <td>
                <code>{f.fact}</code>
              </td>
              <td>{f.consequence}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <H n="06">Known Gaps &amp; Discrepancies</H>
      {ANALYSIS.gaps.map((t) => (
        <div className="archx-note warn" key={t}>
          <Rich text={t} />
        </div>
      ))}

      <H n="07">This Explorer</H>
      <p className="archx-an-p">
        {MODEL.components.length} components across {MODEL.layers.length} layers ·{' '}
        {MODEL.connections.length} connectors · {MODEL.workflows.length} workflows ({steps} steps) ·{' '}
        {MODEL.entities.length} modelled entities · {MODEL.stateMachines.length} state machines.
      </p>
      <p className="archx-an-p">
        <Rich text={ANALYSIS.help} />
      </p>
    </div>
  );
}
