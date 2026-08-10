/**
 * Client-side upload checks, run before bytes are sent.
 *
 * A convenience filter for obvious mistakes only. The authoritative validation is
 * the backend's magic-byte and size check, which this does not replace.
 */
export const MAX_FILE_SIZE_BYTES = 25 * 1024 * 1024;

/** Whether a filename looks like a PDF. */
export function isPdf(name: string, type?: string): boolean {
  return name.toLowerCase().endsWith('.pdf') || type === 'application/pdf';
}

export interface ValidationError {
  title: string;
  description: string;
}

export const NON_PDF_ERROR: ValidationError = {
  title: 'Upload Error',
  description: 'Only PDF files are allowed.',
};

/** Error text for a file over the size limit. */
export function tooLargeError(names: string[]): ValidationError {
  return { title: 'File Too Large', description: `${names.join(', ')} exceeds the 25MB limit.` };
}

/**
 * Filters dropped files to those worth uploading.
 *
 * A convenience check for obvious mistakes only -- the authoritative validation
 * is the backend's magic-byte and size check.
 */
export function validateDroppedFiles(files: File[]): ValidationError | null {
  if (files.some((file) => !isPdf(file.name, file.type))) return NON_PDF_ERROR;
  const tooLarge = files.find((file) => file.size > MAX_FILE_SIZE_BYTES);
  return tooLarge ? tooLargeError([tooLarge.name]) : null;
}

/** Filename without its directory path. */
export function basename(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}
