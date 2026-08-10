import { describe, it, expect } from 'vitest';
import { evidenceDescription } from '@/components/transactions/evidenceDescription';

describe('evidenceDescription', () => {
  it('reports merged sources', () => {
    expect(evidenceDescription('merged').label).toBe('Merged Sources');
  });

  it('reports statement extraction', () => {
    expect(evidenceDescription('statement').label).toBe('Statement Extraction');
  });

  it('reports manual entry', () => {
    expect(evidenceDescription('manual').label).toBe('Manual Entry');
  });

  it.each(['email', 'gmail'])('reports email extraction for %s', (mix) => {
    expect(evidenceDescription(mix).label).toBe('Email Extraction');
  });

  it('matches case-insensitively', () => {
    expect(evidenceDescription('GMAIL_ALERT').label).toBe('Email Extraction');
  });

  it('prefers merged over the other markers when a mix contains several', () => {
    // A reconciled record's source_mix names every contributing source; the
    // merged label is the one that describes the record as a whole.
    expect(evidenceDescription('gmail+statement+merged').label).toBe('Merged Sources');
  });

  it('echoes an unrecognised mix as the detail so it stays diagnosable', () => {
    const result = evidenceDescription('carrier_pigeon');
    expect(result.label).toBe('Unknown Source');
    expect(result.detail).toBe('carrier_pigeon');
  });

  it.each([null, ''])('describes %p as having no source information', (mix) => {
    const result = evidenceDescription(mix);
    expect(result.label).toBe('Unknown Source');
    expect(result.detail).toBe('No source information recorded');
  });
});
