/**
 * Shared prop shapes for the inspector's form cards.
 */
import type { useInstrumentForm } from '../useInstrumentForm';

type Form = ReturnType<typeof useInstrumentForm>;

export interface InstrumentFormProps {
  fields: Form['fields'];
  setField: Form['setField'];
  onSave: () => void;
}
