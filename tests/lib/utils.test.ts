import { describe, it, expect } from 'vitest';
import { cn } from '@/lib/utils';

describe('cn', () => {
  it('joins simple class names', () => {
    expect(cn('a', 'b')).toBe('a b');
  });

  it('drops falsy values', () => {
    expect(cn('a', false, undefined, null, 'b')).toBe('a b');
  });

  it('resolves conflicting Tailwind utility classes, keeping the last one', () => {
    // twMerge should keep only the last conflicting padding utility rather
    // than emitting both (which would let CSS source order silently decide).
    expect(cn('p-2', 'p-4')).toBe('p-4');
  });

  it('applies conditional classes from an object form', () => {
    expect(cn('base', { active: true, disabled: false })).toBe('base active');
  });
});
