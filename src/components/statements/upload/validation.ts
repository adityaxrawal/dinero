/**
 * Client-side upload checks, run before paths are sent.
 *
 * A convenience filter for obvious mistakes only. The authoritative validation is
 * the backend's magic-byte and size check, which this does not replace.
 */

/** Whether a filename looks like a PDF. */
export function isPdf(name: string): boolean {
  return name.toLowerCase().endsWith('.pdf');
}

export interface ValidationError {
  title: string;
  description: string;
}

export const NON_PDF_ERROR: ValidationError = {
  title: 'Upload Error',
  description: 'Only PDF files are allowed.',
};
