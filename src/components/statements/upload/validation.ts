// TASK-FE-012: no backend size limit exists on statements_upload -- this is
// a client-side-only "immediate feedback before even attempting the round
// trip" guard, not a server-enforced rule. 25MB comfortably exceeds any
// real bank statement PDF (typically well under 5MB).
export const MAX_FILE_SIZE_BYTES = 25 * 1024 * 1024;

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

export function tooLargeError(names: string[]): ValidationError {
  return { title: 'File Too Large', description: `${names.join(', ')} exceeds the 25MB limit.` };
}

/** Dropped `File` objects carry name/type/size, so both checks are synchronous. */
export function validateDroppedFiles(files: File[]): ValidationError | null {
  if (files.some((file) => !isPdf(file.name, file.type))) return NON_PDF_ERROR;
  const tooLarge = files.find((file) => file.size > MAX_FILE_SIZE_BYTES);
  return tooLarge ? tooLargeError([tooLarge.name]) : null;
}

export function basename(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}
