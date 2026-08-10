/**
 * Autocomplete source of existing tag names.
 *
 * Suggesting existing tags is what stops near-duplicate tags accumulating.
 */
import React from 'react';

interface Tag {
  id: string;
  name: string;
}

interface TagDatalistProps {
  id: string;
  tags: string[];
  availableTags: Tag[];
}

/** Autocomplete over existing tags, which stops near-duplicates accumulating. */
export function TagDatalist({ id, tags, availableTags }: TagDatalistProps) {
  return (
    <datalist id={id}>
      {availableTags
        .filter((t) => !tags.includes(t.name))
        .map((t) => (
          <option key={t.id} value={t.name} />
        ))}
    </datalist>
  );
}
