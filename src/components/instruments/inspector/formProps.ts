import type { useInstrumentForm } from '../useInstrumentForm';

type Form = ReturnType<typeof useInstrumentForm>;

/** What every editable card in the details tab needs. */
export interface InstrumentFormProps {
  fields: Form['fields'];
  setField: Form['setField'];
  onSave: () => void;
}
