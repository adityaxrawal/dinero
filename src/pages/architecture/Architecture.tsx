/**
 * Architecture — interactive system-design explorer for Dinero.
 *
 * A developer-only screen (compiled out of release builds, same policy as the
 * Debug console) presenting the verified architecture model in the app's
 * champagne/emerald theme: layer-clustered graph, component inspector, end-to-end
 * workflow player, and dedicated data-flow / database / dependency /
 * state-machine views. The standalone `docs/Dinero_Architecture.html` mirrors
 * this screen exactly.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useViewport, useReactFlow, ReactFlowProvider } from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import './architecture.css';
import {
  MODEL,
  LAYER_DISPLAY,
  LAYER_DISPLAY_BY_ID,
  EDGE_KIND_DISPLAY,
  NODE_KIND_LABEL,
  INVARIANTS,
  COMPONENT_BY_ID,
  nodeCode,
  nodeKindLabel,
  type LayerId,
  type EntityDef,
  type StageId,
} from './data';
import { neighbourhood, searchComponents } from './layout';
import { KindGlyph } from './nodes';

import { SystemMapView } from './views/SystemMapView';
import { DataFlowView, StageDetail } from './views/DataFlowView';
import { WorkflowView } from './views/WorkflowView';
import { DatabaseView } from './views/DatabaseView';
import { StateMachineView, StateMachineDetail } from './views/StateMachineView';
import { AnalysisPanel } from './AnalysisPanel';
import { WorkflowPlayer } from './WorkflowPlayer';

type ViewId = 'map' | 'flow' | 'workflows' | 'database' | 'deps' | 'states';

const VIEWS: { id: ViewId; label: string }[] = [
  { id: 'map', label: 'Architecture' },
  { id: 'flow', label: 'Data Flow' },
  { id: 'workflows', label: 'Workflows' },
  { id: 'database', label: 'Database' },
  { id: 'deps', label: 'Impact' },
  { id: 'states', label: 'State Machines' },
];

function ExplorerInner() {
  const [view, setView] = useState<ViewId>('map');
  const [query, setQuery] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [hiddenLayers, setHiddenLayers] = useState<Set<LayerId>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedEntity, setSelectedEntity] = useState<EntityDef | null>(null);
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [labelsOn, setLabelsOn] = useState(false);
  const [showMinimap, setShowMinimap] = useState(true);
  // Side panels start open only on wide windows; below that the canvas wins.
  const [railOpen, setRailOpen] = useState(() => window.innerWidth >= 1500);
  const [inspectorOpen, setInspectorOpen] = useState(() => window.innerWidth >= 1500);
  // Bumped on every rail click so re-clicking the same layer re-frames it.
  const [layerFocus, setLayerFocus] = useState<{ id: LayerId; seq: number } | null>(null);
  const [impactOn, setImpactOn] = useState(false);
  const [workflowId, setWorkflowId] = useState<string | null>(null);
  const [stepIndex, setStepIndex] = useState(0);
  const [machineId, setMachineId] = useState<string>(MODEL.stateMachines[0].id);
  const [flowStage, setFlowStage] = useState<StageId | null>(null);
  const [detailTab, setDetailTab] = useState<'analysis' | 'component'>('analysis');
  const searchRef = useRef<HTMLInputElement>(null);

  const hits = useMemo(() => searchComponents(query), [query]);
  const searchResults = useMemo(() => [...hits].slice(0, 8), [hits]);
  const [activeResult, setActiveResult] = useState(0);

  const stats = useMemo(
    () => [
      { v: MODEL.components.length, k: 'Components' },
      { v: MODEL.connections.length, k: 'Connections' },
      { v: MODEL.layers.length, k: 'Layers' },
      { v: MODEL.workflows.reduce((a, w) => a + w.steps.length, 0), k: 'Workflow steps' },
    ],
    []
  );

  /** The focus set: selection, hover, workflow step, or impact blast radius. */
  const focus = useMemo(() => {
    const impact = impactOn || view === 'deps';
    if (impact && selected) return neighbourhood(selected, 3, 'downstream');
    // A selection keeps its neighbourhood lit; hover previews only otherwise.
    const anchor = selected && !workflowId ? selected : hoverId;
    if (workflowId) {
      const wf = MODEL.workflows.find((w) => w.id === workflowId);
      const step = wf?.steps[Math.min(stepIndex, (wf?.steps.length ?? 1) - 1)];
      if (step) {
        const nodes = new Set(step.components);
        const edges = new Set<string>();
        for (const c of MODEL.connections)
          if (nodes.has(c.from) && nodes.has(c.to)) edges.add(c.id);
        return { nodes, edges };
      }
      return null;
    }
    if (anchor) return neighbourhood(anchor, 1, 'both');
    return null;
  }, [hoverId, selected, impactOn, workflowId, stepIndex, view]);

  const select = useCallback((id: string) => {
    setSelectedEntity(null);
    setSelected(id);
    setDetailTab('component');
    setInspectorOpen(true);
  }, []);

  const pickResult = useCallback(
    (id: string) => {
      select(id);
      setView((v) => (v === 'map' || v === 'deps' ? v : 'map'));
      setSearchOpen(false);
    },
    [select]
  );

  const selectEntity = useCallback((id: string) => {
    setSelected(null);
    setSelectedEntity(MODEL.entities.find((e) => e.id === id) ?? null);
    setDetailTab('component');
    setInspectorOpen(true);
  }, []);

  const focusLayer = useCallback((id: LayerId) => {
    setView((v) => (v === 'map' || v === 'deps' ? v : 'map'));
    setLayerFocus((prev) => ({ id, seq: (prev?.seq ?? 0) + 1 }));
  }, []);

  /** Impact is meaningless without a selection, so it is cleared with it. */
  const clearSelection = useCallback(() => {
    setSelected(null);
    setSelectedEntity(null);
    setImpactOn(false);
  }, []);

  const resetAll = useCallback(() => {
    setQuery('');
    setHiddenLayers(new Set());
    setSelected(null);
    setSelectedEntity(null);
    setHoverId(null);
    setImpactOn(false);
    setLabelsOn(false);
    setWorkflowId(null);
    setStepIndex(0);
    setFlowStage(null);
    setDetailTab('analysis');
  }, []);

  const openWorkflow = useCallback(
    (id: string) => {
      setWorkflowId(id);
      setStepIndex(0);
      setView('workflows');
      clearSelection();
    },
    [clearSelection]
  );

  // Keyboard: "/" focuses search; Escape clears search then selection.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (e.key === '/' && tag !== 'INPUT' && tag !== 'TEXTAREA') {
        e.preventDefault();
        searchRef.current?.focus();
        setSearchOpen(true);
      }
      if (e.key === 'Escape') {
        if (searchOpen) {
          setSearchOpen(false);
          searchRef.current?.blur();
          return;
        }
        clearSelection();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [searchOpen, clearSelection]);

  const component = selected ? COMPONENT_BY_ID[selected] : undefined;
  const impactMode = impactOn || view === 'deps';
  // The Workflows tab always shows a workflow; with none chosen it is the first.
  const shownWorkflowId = workflowId ?? MODEL.workflows[0].id;
  // With nothing selected on a component-based view, the Component tab would
  // only show a placeholder, so fall back to the analysis pane.
  const hasTabDetail = view === 'flow' || view === 'states' || !!component || !!selectedEntity;
  const showAnalysisTab = detailTab === 'analysis' || !hasTabDetail;

  return (
    <div className={'archx' + (focus ? ' focus-on' : '')}>
      <div className="sr-only" aria-live="polite">
        {component
          ? `Selected ${component.name}`
          : selectedEntity
            ? `Selected table ${selectedEntity.name}`
            : ''}
      </div>
      {/* ── top bar ── */}
      <header className="archx-top">
        <div className="archx-brand">
          <div className="archx-mark" aria-hidden />
          <h1>
            Dinero <span>//</span> Architecture
          </h1>
          <div className="sub">Local-First Financial Engine</div>
        </div>

        <div
          className="archx-search"
          onBlur={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setSearchOpen(false);
          }}
        >
          <span className="ic" aria-hidden>
            ⌕
          </span>
          <input
            id="search"
            ref={searchRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSearchOpen(true);
              setActiveResult(0);
            }}
            onFocus={() => setSearchOpen(true)}
            // Combobox keyboard model: arrows move the active option, Enter picks it.
            onKeyDown={(e) => {
              if (!searchResults.length) return;
              if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                e.preventDefault();
                const step = e.key === 'ArrowDown' ? 1 : -1;
                setActiveResult((i) => (i + step + searchResults.length) % searchResults.length);
              } else if (e.key === 'Enter') {
                e.preventDefault();
                pickResult(searchResults[Math.min(activeResult, searchResults.length - 1)]);
              }
            }}
            placeholder="Search components, layers, technology…"
            autoComplete="off"
            aria-label="Search architecture"
            role="combobox"
            aria-expanded={searchOpen && !!query.trim()}
            aria-controls="results"
            aria-autocomplete="list"
            aria-activedescendant={
              searchOpen && searchResults.length ? `sr-${activeResult}` : undefined
            }
          />
          <span className="kbd">/</span>
          <div
            id="results"
            className={'archx-results' + (searchOpen && query.trim() ? ' open' : '')}
            role="listbox"
          >
            {searchOpen && query.trim() && (
              <>
                {searchResults.length === 0 && (
                  <div className="r" style={{ color: 'var(--ax-text-3)' }}>
                    No matches
                  </div>
                )}
                {searchResults.map((id, i) => {
                  const c = COMPONENT_BY_ID[id];
                  return (
                    <div
                      key={id}
                      id={`sr-${i}`}
                      className={'r' + (i === activeResult ? ' act' : '')}
                      role="option"
                      aria-selected={i === activeResult}
                      onMouseDown={(e) => e.preventDefault()}
                      onMouseEnter={() => setActiveResult(i)}
                      onClick={() => pickResult(id)}
                    >
                      <span className="id">{nodeCode(id)}</span>
                      <span className="nm">{c?.name}</span>
                      <span className="sec">{LAYER_DISPLAY_BY_ID[c?.layer ?? '']?.name}</span>
                    </div>
                  );
                })}
                {hits.size > searchResults.length && (
                  <div className="r" style={{ color: 'var(--ax-text-3)' }}>
                    +{hits.size - searchResults.length} more highlighted on the map
                  </div>
                )}
              </>
            )}
          </div>
        </div>

        <div className="archx-toolbar">
          <button
            className={'archx-tbtn' + (railOpen ? ' on' : '')}
            onClick={() => setRailOpen((v) => !v)}
            aria-pressed={railOpen}
            title="Show or hide the navigator (layers, workflows, legend)"
          >
            <svg viewBox="0 0 24 24">
              <rect x="3" y="4" width="18" height="16" rx="2" />
              <path d="M9 4v16" />
            </svg>
            <span className="tl">Navigator</span>
          </button>
          <button
            className={'archx-tbtn' + (inspectorOpen ? ' on' : '')}
            onClick={() => setInspectorOpen((v) => !v)}
            aria-pressed={inspectorOpen}
            title="Show or hide the inspector"
          >
            <svg viewBox="0 0 24 24">
              <rect x="3" y="4" width="18" height="16" rx="2" />
              <path d="M15 4v16" />
            </svg>
            <span className="tl">Inspector</span>
          </button>
          <button
            id="btnLabels"
            className={'archx-tbtn' + (labelsOn ? ' on' : '')}
            onClick={() => setLabelsOn(!labelsOn)}
            title="Toggle connection labels"
          >
            <svg viewBox="0 0 24 24">
              <path d="M4 7h16M4 12h10M4 17h13" />
            </svg>
            <span className="tl">Labels</span>
          </button>
          <button
            id="btnImpact"
            className={'archx-tbtn' + (impactMode && selected ? ' on' : '')}
            onClick={() => setImpactOn(!impactOn)}
            title={
              view === 'deps'
                ? 'Always on in the Impact view'
                : 'Impact analysis: highlight the downstream blast radius (3 hops)'
            }
            disabled={!selected || view === 'deps'}
          >
            <svg viewBox="0 0 24 24">
              <circle cx="12" cy="12" r="3" />
              <circle cx="12" cy="12" r="8" />
            </svg>
            <span className="tl">Impact</span>
          </button>
          <button
            id="btnReset"
            className="archx-tbtn"
            onClick={resetAll}
            title="Reset view & filters"
          >
            <svg viewBox="0 0 24 24">
              <path d="M4 12a8 8 0 1 1 2.5 5.8" />
              <path d="M4 6v6h6" />
            </svg>
            <span className="tl">Reset</span>
          </button>
        </div>
      </header>

      {/* ── body ── */}
      <div className="archx-body">
        {/* left rail */}
        {railOpen && (
          <aside className="archx-rail" aria-label="Architecture navigation">
            <div className="archx-sec">
              <h2 className="archx-h">
                System at a Glance <span className="n">DINERO</span>
              </h2>
              <div id="stats" className="archx-stats">
                {stats.map((s) => (
                  <div className="archx-stat" key={s.k}>
                    <div className="v">{s.v}</div>
                    <div className="k">{s.k}</div>
                  </div>
                ))}
              </div>
            </div>

            <div className="archx-sec">
              <h2 className="archx-h">
                Architecture Layers <span className="n">{LAYER_DISPLAY.length}</span>
              </h2>
              <div id="layers">
                {LAYER_DISPLAY.map((l) => {
                  const off = hiddenLayers.has(l.id as LayerId);
                  const count = MODEL.components.filter((c) => c.layer === l.id).length;
                  const toggle = () =>
                    setHiddenLayers((prev) => {
                      const next = new Set(prev);
                      if (next.has(l.id as LayerId)) next.delete(l.id as LayerId);
                      else next.add(l.id as LayerId);
                      return next;
                    });
                  return (
                    <div key={l.id} className={'archx-layer-row' + (off ? ' off' : '')}>
                      <button
                        className="archx-layer"
                        onClick={() => focusLayer(l.id as LayerId)}
                        disabled={off}
                        title={`${l.tech} · click to focus`}
                      >
                        <span className="dot" style={{ background: l.color }} />
                        <span className="lid">{l.code}</span>
                        <span className="lnm">{l.name}</span>
                        <span className="lc">{count}</span>
                      </button>
                      <button
                        className="archx-layer-eye"
                        onClick={toggle}
                        aria-pressed={!off}
                        aria-label={`${off ? 'Show' : 'Hide'} ${l.name}`}
                        title={off ? 'Show layer' : 'Hide layer'}
                      >
                        <svg viewBox="0 0 24 24" aria-hidden>
                          <path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12z" />
                          {off ? <path d="M4 4l16 16" /> : <circle cx="12" cy="12" r="2.8" />}
                        </svg>
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="archx-sec">
              <h2 className="archx-h">
                Workflows{' '}
                <span className="n">{String(MODEL.workflows.length).padStart(2, '0')}</span>
              </h2>
              <div id="wflist">
                {MODEL.workflows.map((w) => (
                  <button
                    key={w.id}
                    className={
                      'archx-wf' +
                      (workflowId === w.id || (view === 'workflows' && shownWorkflowId === w.id)
                        ? ' on'
                        : '')
                    }
                    onClick={() => openWorkflow(w.id)}
                  >
                    <div className="wt">{w.name}</div>
                    <div className="wd">
                      {w.summary.length > 110 ? `${w.summary.slice(0, 110)}…` : w.summary}
                    </div>
                    <div className="wm">
                      <span>{String(w.steps.length).padStart(2, '0')} steps</span>
                      <span>{w.participants.length} participants</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>

            <div className="archx-sec archx-inv">
              <h2 className="archx-h">Privacy Invariants</h2>
              <div id="inv">
                {INVARIANTS.map((iv) => (
                  <div key={iv.title}>
                    <div className="it">{iv.title}</div>
                    <div className="ib">{iv.body}</div>
                  </div>
                ))}
              </div>
            </div>

            <div className="archx-sec">
              <h2 className="archx-h">Legend</h2>
              <div id="legend">
                {Object.entries(EDGE_KIND_DISPLAY).map(([k, v]) => (
                  <div className="archx-legend" key={k}>
                    <svg viewBox="0 0 30 8" aria-hidden>
                      <path
                        d="M0 4h30"
                        stroke={v.color}
                        strokeWidth="2"
                        strokeDasharray={v.dash || undefined}
                      />
                    </svg>
                    {v.label}
                  </div>
                ))}
                <div className="archx-legend">
                  <div className="sq" style={{ background: 'var(--ax-card)' }} /> Component · left
                  bar = layer
                </div>
                <div className="archx-legend">
                  <div
                    className="sq"
                    style={{ background: 'color-mix(in srgb, var(--ax-ink) 5%, var(--ax-bg))' }}
                  />{' '}
                  Layer cluster box
                </div>
                <div className="archx-legend">
                  <div className="sq" style={{ borderStyle: 'dashed', borderWidth: 1.5 }} />{' '}
                  External trust boundary
                </div>
                <div className="archx-legend archx-legend-kinds">
                  {Object.keys(NODE_KIND_LABEL).map((kind) => (
                    <div className="kind" key={kind}>
                      <div className={`archx-node k-${kind}`} aria-hidden />
                      <KindGlyph kind={kind} />
                      {nodeKindLabel(kind)}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </aside>
        )}

        {/* stage */}
        <main className="archx-stage">
          {/* view name + hint as a real header row — never overlays the canvas */}
          <div className="archx-modechip">
            <b id="modeName">{VIEWS.find((v) => v.id === view)?.label}</b>
            <span id="modeHint">
              {impactMode
                ? selected
                  ? 'impact: downstream blast radius · 3 hops'
                  : 'select a component to see its downstream blast radius (3 hops)'
                : selected
                  ? 'click empty space or esc to clear'
                  : 'click a component to explore'}
            </span>
            {(view === 'map' || view === 'deps') && (
              <span className="counts">
                {LAYER_DISPLAY.length - hiddenLayers.size} layers · {MODEL.components.length}{' '}
                components · {MODEL.connections.length} connectors
              </span>
            )}
          </div>
          <div className="archx-stagebody">
            {(view === 'map' || view === 'deps') && (
              <SystemMapView
                hiddenLayers={hiddenLayers}
                selectedId={selected}
                searchHits={hits}
                labelsOn={labelsOn}
                focus={focus}
                showMinimap={showMinimap}
                layerFocus={layerFocus}
                onSelect={select}
                onClearSelection={clearSelection}
                onHover={setHoverId}
              />
            )}
            {view === 'flow' && (
              <DataFlowView
                selectedStage={flowStage}
                onSelectStage={(id) => {
                  setFlowStage(id);
                  setDetailTab('component');
                }}
              />
            )}
            {view === 'workflows' && (
              <WorkflowView
                workflowId={shownWorkflowId}
                activeStep={workflowId ? stepIndex : null}
                onStep={(i) => {
                  setWorkflowId(shownWorkflowId);
                  setStepIndex(i);
                }}
                onSelectComponent={select}
              />
            )}
            {view === 'database' && (
              <DatabaseView
                selectedId={selectedEntity?.id ?? null}
                labelsOn={labelsOn}
                onSelect={selectEntity}
              />
            )}
            {view === 'states' && (
              <StateMachineView machineId={machineId} onSelectMachine={setMachineId} />
            )}
          </div>
          {/* view tabs + zoom controls as a real footer bar — never overlays the canvas */}
          <div className="archx-stagefoot">
            <div className="archx-views" role="tablist" aria-label="Architecture views">
              {VIEWS.map((v) => (
                <button
                  key={v.id}
                  className={'archx-vbtn' + (view === v.id ? ' on' : '')}
                  onClick={() => {
                    setView(v.id);
                    // These tabs keep their detail in the inspector's second tab.
                    if (v.id === 'flow' || v.id === 'states') setDetailTab('component');
                  }}
                  role="tab"
                  aria-selected={view === v.id}
                >
                  {v.label}
                </button>
              ))}
            </div>
            {view !== 'workflows' && (
              <ZoomControls
                minimap={
                  view === 'map' || view === 'deps'
                    ? { on: showMinimap, toggle: () => setShowMinimap((v) => !v) }
                    : undefined
                }
              />
            )}
          </div>
        </main>

        {/* right detail */}
        {inspectorOpen && (
          <aside className="archx-detail" aria-label="Inspector">
            <div className="archx-dtabs">
              <button
                className={'archx-dtab' + (showAnalysisTab ? ' on' : '')}
                onClick={() => setDetailTab('analysis')}
              >
                Architecture Analysis
              </button>
              <button
                className={'archx-dtab' + (!showAnalysisTab ? ' on' : '')}
                onClick={() => setDetailTab('component')}
              >
                {view === 'flow' ? 'Stage' : view === 'states' ? 'Machine' : 'Component'}
              </button>
            </div>
            {/* focusable so keyboard users can scroll a text-only pane */}
            <div id="dcontent" className="archx-dcontent" tabIndex={0}>
              {showAnalysisTab ? (
                <AnalysisPanel />
              ) : view === 'states' ? (
                <StateMachineDetail machineId={machineId} />
              ) : view === 'flow' ? (
                <StageDetail
                  selectedStage={flowStage}
                  onSelectComponent={(id) => {
                    select(id);
                    setView('map');
                  }}
                />
              ) : component ? (
                <ComponentDetail
                  id={selected!}
                  onSelect={select}
                  onOpenWorkflow={(id) => openWorkflow(id)}
                />
              ) : selectedEntity ? (
                <EntityDetail entity={selectedEntity} />
              ) : (
                <div className="archx-dp">
                  <p className="archx-an-p">
                    Select a component on the map, or an entity in the Database view, to inspect it
                    here.
                  </p>
                </div>
              )}
            </div>
          </aside>
        )}
      </div>

      {/* workflow player */}
      <WorkflowPlayer
        workflowId={workflowId}
        stepIndex={stepIndex}
        onStep={(i) => {
          const wf = workflowId ? MODEL.workflows.find((w) => w.id === workflowId) : null;
          if (!wf) return;
          const clamped = Math.max(0, Math.min(i, wf.steps.length - 1));
          // Stepping highlights the step's components on the canvas; it does
          // not select one, so the inspector is left alone.
          setStepIndex(clamped);
        }}
        onClose={() => {
          setWorkflowId(null);
          setStepIndex(0);
        }}
      />
    </div>
  );
}

/** Live zoom read-out + zoom controls, mirroring the standalone document. */
function ZoomControls({ minimap }: { minimap?: { on: boolean; toggle: () => void } | undefined }) {
  const viewport = useViewport();
  const { zoomIn, zoomOut, fitView } = useReactFlow();
  const pct = Math.round((viewport.zoom ?? 1) * 100);
  return (
    <div className="archx-stagebar">
      <div
        className="archx-zoom"
        title="Drag or scroll to pan · pinch or ⌘-scroll to zoom"
        aria-label={`Zoom ${pct} percent`}
      >
        <span id="zoomval">{pct}</span>%
      </div>
      <button
        id="zIn"
        className="archx-sbtn"
        onClick={() => zoomIn({ duration: 180 })}
        title="Zoom in"
        aria-label="Zoom in"
      >
        <svg viewBox="0 0 24 24">
          <path d="M12 5v14M5 12h14" />
        </svg>
      </button>
      <button
        id="zOut"
        className="archx-sbtn"
        onClick={() => zoomOut({ duration: 180 })}
        title="Zoom out"
        aria-label="Zoom out"
      >
        <svg viewBox="0 0 24 24">
          <path d="M5 12h14" />
        </svg>
      </button>
      <button
        id="zFit"
        className="archx-sbtn"
        onClick={() => fitView({ duration: 220 })}
        title="Fit to screen"
        aria-label="Fit to screen"
      >
        <svg viewBox="0 0 24 24">
          <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
        </svg>
      </button>
      {minimap && (
        <button
          className={'archx-sbtn' + (minimap.on ? ' on' : '')}
          onClick={minimap.toggle}
          title={minimap.on ? 'Hide minimap' : 'Show minimap'}
          aria-label="Minimap"
          aria-pressed={minimap.on}
        >
          <svg viewBox="0 0 24 24">
            <rect x="3" y="5" width="18" height="14" rx="1.5" />
            <rect x="12" y="11" width="6" height="5" />
          </svg>
        </button>
      )}
    </div>
  );
}

/** Component inspector — the reference explorer's Component tab. */
function ComponentDetail({
  id,
  onSelect,
  onOpenWorkflow,
}: {
  id: string;
  onSelect: (id: string) => void;
  onOpenWorkflow: (id: string) => void;
}) {
  const c = COMPONENT_BY_ID[id];
  const layer = LAYER_DISPLAY_BY_ID[c.layer];
  const { inbound, outbound } = useMemo(() => {
    type Link = { name: string; id: string; via: string; kind: string; key: string };
    const inb: Link[] = [];
    const out: Link[] = [];
    for (const conn of MODEL.connections) {
      const base = { via: conn.label, kind: conn.kind, key: conn.id };
      if (conn.to === id && COMPONENT_BY_ID[conn.from])
        inb.push({ ...base, name: COMPONENT_BY_ID[conn.from].name, id: conn.from });
      if (conn.from === id && COMPONENT_BY_ID[conn.to])
        out.push({ ...base, name: COMPONENT_BY_ID[conn.to].name, id: conn.to });
    }
    return { inbound: inb, outbound: out };
  }, [id]);
  const linkGroup = (title: string, items: typeof inbound) =>
    items.length > 0 && (
      <>
        <div className="archx-sub">{title}</div>
        <div className="archx-dtab-links">
          {items.map((x) => (
            <button
              className="archx-dlink"
              key={x.key}
              onClick={() => onSelect(x.id)}
              title={`${EDGE_KIND_DISPLAY[x.kind]?.label ?? x.kind}: ${x.via}`}
            >
              <span
                className="kd"
                style={{ background: EDGE_KIND_DISPLAY[x.kind]?.color }}
                aria-hidden
              />
              {x.name} <span className="via">· {x.via}</span>
            </button>
          ))}
        </div>
      </>
    );
  const workflows = MODEL.workflows.filter((w) => w.steps.some((s) => s.components.includes(id)));

  return (
    <div className="archx-dp">
      <h3 className="archx-an-h">
        <span className="num" aria-hidden>
          {layer?.code}
        </span>
        {c.name}
      </h3>
      <div className="archx-chips">
        <span className="archx-chip accent">{nodeKindLabel(c.kind)}</span>
        <span className="archx-chip">{c.confidence}</span>
        <span className="archx-chip">{layer?.name}</span>
      </div>
      <p className="archx-an-p">{c.purpose}</p>
      <p className="archx-an-p">{c.description}</p>

      {c.responsibilities.length > 0 && (
        <>
          <h3 className="archx-an-h">
            <span className="num" aria-hidden>
              ↳
            </span>{' '}
            Responsibilities
          </h3>
          {c.responsibilities.map((r) => (
            <div className="archx-an-li" key={r}>
              {r}
            </div>
          ))}
        </>
      )}

      {c.facts.length > 0 && (
        <>
          <h3 className="archx-an-h">
            <span className="num" aria-hidden>
              ◈
            </span>{' '}
            Operational Facts
          </h3>
          {c.facts.map((f) => (
            <div className="archx-an-li" key={f}>
              {f}
            </div>
          ))}
        </>
      )}

      {c.notes && c.notes.length > 0 && (
        <div className="archx-note warn">
          <b>Known gaps</b>
          {c.notes.map((n) => (
            <div key={n} style={{ marginTop: 5 }}>
              {n}
            </div>
          ))}
        </div>
      )}

      <h3 className="archx-an-h">
        <span className="num" aria-hidden>
          ⇄
        </span>{' '}
        Interfaces
      </h3>
      <div className="archx-kv">
        <span className="k">Consumes</span>
        <span className="v">{c.inputs.join(' · ') || '—'}</span>
        <span className="k">Produces</span>
        <span className="v">{c.outputs.join(' · ') || '—'}</span>
        <span className="k">Surface</span>
        <span className="v">{c.apiSurface.join(' · ') || '—'}</span>
        <span className="k">Source</span>
        <span className="v">
          {c.sources.length === 0
            ? '—'
            : c.sources.map((src) => (
                <button
                  key={src}
                  className="archx-src"
                  onClick={() => void navigator.clipboard?.writeText(src)}
                  title="Copy path"
                >
                  {src}
                </button>
              ))}
        </span>
      </div>
      {c.technologies.length > 0 && (
        <div className="archx-chips">
          {c.technologies.map((t) => (
            <span className="archx-chip" key={t}>
              {t}
            </span>
          ))}
        </div>
      )}

      <h3 className="archx-an-h">
        <span className="num" aria-hidden>
          ⇱
        </span>{' '}
        Connections
      </h3>
      {linkGroup('Inbound — sends to this', inbound)}
      {linkGroup('Outbound — this sends to', outbound)}

      {workflows.length > 0 && (
        <>
          <h3 className="archx-an-h">
            <span className="num" aria-hidden>
              ▶
            </span>{' '}
            Participates In
          </h3>
          <div className="archx-dtab-links">
            {workflows.map((w) => (
              <button className="archx-dlink" key={w.id} onClick={() => onOpenWorkflow(w.id)}>
                {w.name}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** Entity inspector for the database view. */
function EntityDetail({ entity }: { entity: EntityDef }) {
  return (
    <div className="archx-dp">
      <h3 className="archx-an-h">
        <span className="num" aria-hidden>
          ▦
        </span>
        {entity.name}
      </h3>
      <div className="archx-chips">
        <span className="archx-chip accent">{entity.group}</span>
      </div>
      <p className="archx-an-p">{entity.description}</p>
      <h3 className="archx-an-h">
        <span className="num" aria-hidden>
          ↳
        </span>{' '}
        Columns
      </h3>
      <table className="archx-table">
        <thead>
          <tr>
            <th>Key</th>
            <th>Column</th>
            <th>Type</th>
            <th>Note</th>
          </tr>
        </thead>
        <tbody>
          {entity.columns.map((col) => (
            <tr key={col.name}>
              <td>{col.key ?? ''}</td>
              <td>
                <code>{col.name}</code>
              </td>
              <td>{col.type}</td>
              <td>{col.note ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function Architecture() {
  return (
    <ReactFlowProvider>
      <ExplorerInner />
    </ReactFlowProvider>
  );
}
