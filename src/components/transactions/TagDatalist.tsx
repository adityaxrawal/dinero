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
