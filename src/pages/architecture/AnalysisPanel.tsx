/**
 * Architecture Analysis pane — the explorer's default detail content.
 *
 * Covers the system's shape, its recurring patterns, the privacy invariants
 * that govern every design decision, the operational profile, and the known
 * gaps the verification surfaced. Mirrors the standalone document's analysis
 * section so both surfaces tell the same story.
 */
import { MODEL, INVARIANTS, SCHEMA_FACTS } from './data';

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

export function AnalysisPanel() {
  const steps = MODEL.workflows.reduce((a, w) => a + w.steps.length, 0);
  return (
    <div className="archx-dp">
      <H n="01">System Shape</H>
      <p className="archx-an-p">
        <b>Dinero</b> is a local-first macOS personal-finance tracker: a Tauri v2 application (Rust
        core + React 19 frontend) that ingests bank/UPI transaction alerts and PDF statements from
        Gmail — or manual upload — extracts and reconciles them into one unified ledger{' '}
        <b>entirely on-device</b>, paired with a minimal separate cloud service that handles{' '}
        <b>only</b> licensing and billing.
      </p>
      <p className="archx-an-p">
        Two ingestion channels converge on three routing queues behind a single license write-gate,
        with a fourth queue carrying deferred LLM work downstream; every queue funnels into one
        reconciliation engine that produces the canonical ledger the React frontend renders. A dozen
        background workers keep the system self-healing without ever leaving the device.
      </p>

      <H n="02">Recurring Patterns</H>
      <div className="archx-an-li">
        <b>One processing path per job type</b> — historical scan, live polling and manual upload
        all converge on the same queues and the same Gate 1→2→3 / password-resolution logic.
      </div>
      <div className="archx-an-li">
        <b>Async acknowledgment, event-driven outcome</b> — expensive operations return immediately
        and report results via named events; the UI never blocks on multi-minute work.
      </div>
      <div className="archx-an-li">
        <b>Checkpoint everything long-running</b> — scans, Gmail history cursors and statement
        drafts are durably resumable across crashes.
      </div>
      <div className="archx-an-li">
        <b>Precision over recall</b> — Gate 3 never accepts a partial extraction; the Instrument
        Gate never defaults a missing identifier; a wrong PDF password re-prompts without losing
        context.
      </div>

      <H n="03">Privacy Invariants</H>
      {INVARIANTS.map((iv) => (
        <div className="archx-note" key={iv.title}>
          <b>{iv.title}</b>
          <div style={{ marginTop: 5 }}>{iv.body}</div>
        </div>
      ))}

      <H n="04">Operational Profile</H>
      <div className="archx-kv">
        <span className="k">Queues</span>
        <span className="v">
          Transaction 4w/256 · Statement 5-permit/64 · Mandate 1w/64 · Layer-6 6w/256 (durable)
        </span>
        <span className="k">Workers</span>
        <span className="v">
          12 boot-spawned loops (polling 60s, health 60s, maintenance 24h, licensing 6h, updates
          ~6h)
        </span>
        <span className="k">Storage</span>
        <span className="v">
          SQLCipher SQLite — 43 tables + FTS5 search index, 60 migrations, append-only audit + match
          trails
        </span>
        <span className="k">Sidecars</span>
        <span className="v">
          pdf_sidecar (spawn-per-file) · llama_sidecar (long-lived loopback llama-server, b10068)
        </span>
        <span className="k">IPC</span>
        <span className="v">
          142 registered Tauri commands; 37 writes test-enforced through assert_write_allowed
        </span>
        <span className="k">Licensing</span>
        <span className="v">
          8-edge enforced state machine; Locked is terminal; 7-day offline grace
        </span>
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
      <div className="archx-note warn">
        <b>ADR-024 — NetworkClient coverage hole.</b> Three paths bypass the outbound audit trail:
        the Hugging Face GGUF download, the llama.cpp binary download, and the updater check. The
        Network Activity log shows 3 of the 5 disclosed channels.
      </div>
      <div className="archx-note warn">
        <b>IPC command-count discrepancy.</b> The code registers 142 commands in generate_handler!;
        the design docs quote 143 and 151; figures are recorded rather than silently reconciled.
      </div>
      <div className="archx-note warn">
        <b>License state machine.</b> The docs also contain an aspirational transition graph the
        code does not enforce — only the 8-edge graph is real. Locked has no local exit.
      </div>
      <div className="archx-note warn">
        <b>Statement row extraction.</b> All seven bank-specific parsers matched zero of nine real
        statements; <code>parse_universal</code> took the corpus 0→79 rows.
      </div>
      <div className="archx-note warn">
        <b>Polling processing failures</b> leave only a log line — a fetched-but-failed message is
        never re-delivered once its history record is consumed.
      </div>

      <H n="07">This Explorer</H>
      <p className="archx-an-p">
        {MODEL.components.length} components across {MODEL.layers.length} layers ·{' '}
        {MODEL.connections.length} connectors · {MODEL.workflows.length} workflows ({steps} steps) ·{' '}
        {MODEL.entities.length} modelled entities · {MODEL.stateMachines.length} state machines.
      </p>
      <p className="archx-an-p">
        Click any component to inspect it. Use <b>Impact</b> for the downstream blast radius,{' '}
        <b>Labels</b> to pin connector labels, and the workflow rail to step through an end-to-end
        flow — the participating components light up as you go.
      </p>
    </div>
  );
}
