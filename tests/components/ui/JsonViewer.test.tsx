import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { JsonViewer } from '@/components/ui/JsonViewer';

const text = (data: unknown) => {
  const { container } = render(<JsonViewer data={data} />);
  return container.textContent;
};

describe('JsonViewer primitives', () => {
  it('quotes strings', () => {
    expect(text('hello')).toBe('"hello"');
  });

  it('renders numbers bare', () => {
    expect(text(42)).toBe('42');
    expect(text(0)).toBe('0');
    expect(text(-1.5)).toBe('-1.5');
  });

  it.each([
    [true, 'true'],
    [false, 'false'],
  ])('renders %s as %s', (value, expected) => {
    expect(text(value)).toBe(expected);
  });

  it.each([null, undefined])('renders %p as null', (value) => {
    expect(text(value)).toBe('null');
  });

  it('renders an empty string as empty quotes', () => {
    expect(text('')).toBe('""');
  });

  it('colours each primitive type differently', () => {
    const colourOf = (data: unknown) => {
      const { container } = render(<JsonViewer data={data} />);
      return container.querySelector('span')!.className;
    };
    const colours = [colourOf('s'), colourOf(1), colourOf(true), colourOf(null)];
    expect(new Set(colours).size).toBe(4);
  });
});

describe('JsonViewer arrays', () => {
  it('renders an empty array as []', () => {
    expect(text([])).toBe('[]');
  });

  it('lists each element', () => {
    expect(text([1, 2, 3])).toBe('1,2,3');
  });

  it('does not add a trailing comma', () => {
    expect(text(['only'])).toBe('"only"');
  });

  it('recurses into nested arrays', () => {
    expect(text([[1, 2], [3]])).toBe('1,2,3');
  });
});

describe('JsonViewer objects', () => {
  it('renders each key with its value', () => {
    expect(text({ amount: 450 })).toBe('"amount":450');
  });

  it('separates entries with commas but not after the last', () => {
    expect(text({ a: 1, b: 2 })).toBe('"a":1,"b":2');
  });

  it('renders an empty object without content', () => {
    expect(text({})).toBe('');
  });

  it('recurses into nested objects', () => {
    expect(text({ outer: { inner: 'x' } })).toBe('"outer":"inner":"x"');
  });

  it('handles an object holding an array', () => {
    expect(text({ tags: ['a', 'b'] })).toBe('"tags":"a","b"');
  });

  it('renders a realistic bank payload without throwing', () => {
    render(
      <JsonViewer
        data={{
          amount_minor: 45050,
          merchant: 'SWIGGY',
          reference_id: null,
          flags: [true, false],
          nested: { emi: { installments: 6 } },
        }}
      />
    );
    expect(screen.getByText(/"merchant"/)).toBeInTheDocument();
    expect(screen.getByText('"SWIGGY"')).toBeInTheDocument();
  });
});
