/**
 * Turns an observation's provenance into a readable description.
 */
export function evidenceDescription(sourceMix: string | null): { label: string; detail: string } {
  const value = (sourceMix || '').toLowerCase();
  if (value.includes('merged')) {
    return { label: 'Merged Sources', detail: 'Reconciled from multiple matching observations' };
  }
  if (value.includes('statement')) {
    return { label: 'Statement Extraction', detail: 'Parsed from an uploaded/emailed statement' };
  }
  if (value.includes('manual')) {
    return { label: 'Manual Entry', detail: 'Entered directly by you' };
  }
  if (value.includes('email') || value.includes('gmail')) {
    return { label: 'Email Extraction', detail: 'Parsed from a Gmail transaction alert' };
  }
  return { label: 'Unknown Source', detail: sourceMix || 'No source information recorded' };
}
