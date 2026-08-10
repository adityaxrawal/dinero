/**
 * Categorical and sequential chart colours.
 *
 * Slot order is fixed and never cycled, which is what keeps adjacent series
 * distinguishable under colour vision deficiency. Slots that fall below the
 * contrast target always carry a direct label rather than relying on colour.
 */
export const CATEGORICAL_PALETTE = [
  '#2a78d6',
  '#1baf7a',
  '#eda100',
  '#008300',
  '#4a3aa7',
  '#e34948',
  '#e87ba4',
  '#eb6834',
] as const;

export const OTHER_SLICE_COLOR = '#9ca3af';

export const SEQUENTIAL_LINE_COLOR = '#2a78d6';
