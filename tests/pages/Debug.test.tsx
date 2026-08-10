// Covers the Debug page shell -- the ?section= tab routing that decides
// which viewer mounts. Each viewer has (or will have) its own spec; this
// pins the dispatch, which nothing else exercises.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Debug from '@/pages/Debug';

const setSearchParams = vi.fn();
let search = new URLSearchParams();

vi.mock('react-router-dom', () => ({
  useSearchParams: () => [search, setSearchParams],
}));
vi.mock('@/pages/debug/useDebugMetrics', () => ({
  useDebugMetrics: () => ({ metrics: { a: 1 }, ram: null, refresh: vi.fn() }),
}));

vi.mock('@/components/debug/ParseErrorViewer', () => ({
  ParseErrorViewer: () => <div data-testid="parse-errors" />,
}));
vi.mock('@/components/debug/UnprocessedStatementViewer', () => ({
  UnprocessedStatementViewer: () => <div data-testid="unprocessed" />,
}));
vi.mock('@/components/debug/ReconciliationClusterViewer', () => ({
  ReconciliationClusterViewer: () => <div data-testid="clusters" />,
}));
vi.mock('@/components/debug/AuditLogViewer', () => ({
  AuditLogViewer: () => <div data-testid="audit-log" />,
}));
vi.mock('@/components/debug/ReleaseReadinessViewer', () => ({
  ReleaseReadinessViewer: () => <div data-testid="release-readiness" />,
}));
vi.mock('@/pages/debug/PipelineSection', () => ({ default: () => <div data-testid="pipeline" /> }));
vi.mock('@/pages/debug/SystemMetricsSection', () => ({ default: () => <div data-testid="system" /> }));

const at = (section?: string) => {
  search = new URLSearchParams(section ? { section } : {});
};

beforeEach(() => {
  vi.clearAllMocks();
  at();
});

describe('Debug tab routing', () => {
  it('defaults to the pipeline section when the URL carries no ?section=', () => {
    render(<Debug />);
    expect(screen.getByTestId('pipeline')).toBeInTheDocument();
  });

  it.each([
    ['pipeline', ['pipeline']],
    ['extraction', ['parse-errors', 'unprocessed']],
    ['reconciliation', ['clusters']],
    ['audit', ['audit-log']],
    ['release-readiness', ['release-readiness']],
    ['system', ['system']],
  ])('renders the %s section', (section, testIds) => {
    at(section);
    render(<Debug />);
    for (const id of testIds) expect(screen.getByTestId(id)).toBeInTheDocument();
  });

  it('mounts only the active section', () => {
    at('audit');
    render(<Debug />);
    expect(screen.getByTestId('audit-log')).toBeInTheDocument();
    expect(screen.queryByTestId('pipeline')).not.toBeInTheDocument();
    expect(screen.queryByTestId('clusters')).not.toBeInTheDocument();
    expect(screen.queryByTestId('system')).not.toBeInTheDocument();
  });

  it('falls back to pipeline for an unrecognised section', () => {
    at('nonsense');
    render(<Debug />);
    expect(screen.queryByTestId('pipeline')).not.toBeInTheDocument();
    expect(screen.queryByTestId('system')).not.toBeInTheDocument();
  });

  it('writes the chosen tab back to the query string', () => {
    render(<Debug />);
    fireEvent.click(screen.getByRole('button', { name: /Audit Log/ }));
    expect(setSearchParams).toHaveBeenCalledWith({ section: 'audit' });
  });

  it('lists every tab in the sidebar', () => {
    render(<Debug />);
    for (const label of [
      'Pipeline State',
      'Extraction Issues',
      'Reconciliation',
      'Audit Log',
      'System Health',
      'Release Readiness',
    ]) {
      expect(screen.getByRole('button', { name: new RegExp(label) })).toBeInTheDocument();
    }
  });
});
