/**
 * Tag editing for a transaction, with autocomplete over existing tags.
 */
import { Plus, X } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { TagDatalist } from '@/components/transactions/TagDatalist';
import { TagsHeader, EmptyTagsNotice } from '@/components/transactions/TransactionFields';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';

type Form = ReturnType<typeof useTransactionForm>;

interface TagEditorProps {
  tags: Form['tags'];
  availableTags: Form['availableTags'];
  newTag: string;
  setNewTag: (v: string) => void;
  onAddTag: () => void;
  onRemoveTag: (tag: string) => void;
}

/** Tag editing with autocomplete over existing tags. */
export default function TagEditor({
  tags,
  availableTags,
  newTag,
  setNewTag,
  onAddTag,
  onRemoveTag,
}: TagEditorProps) {
  return (
    <div className="space-y-2 pt-1 border-t border-emerald-ink/10">
      <TagsHeader count={tags.length} />
      <div className="flex flex-wrap gap-1.5 min-h-[30px] items-center">
        {tags.length === 0 ? (
          <EmptyTagsNotice />
        ) : (
          tags.map((tag) => (
            <Badge
              key={tag}
              variant="secondary"
              className="flex items-center gap-1 bg-emerald-ink/10 text-emerald-ink hover:bg-emerald-ink/20 rounded-full px-2.5 py-0.5"
            >
              {tag}
              <button
                type="button"
                aria-label={`Remove tag ${tag}`}
                className="hover:bg-emerald-ink/20 p-0.5 rounded-full cursor-pointer"
                onClick={() => onRemoveTag(tag)}
              >
                <X className="w-3 h-3" aria-hidden="true" />
              </button>
            </Badge>
          ))
        )}
      </div>
      <div className="flex gap-2 pt-1">
        <Input
          aria-label="New tag"
          placeholder="Add new tag..."
          list="tag-suggestions"
          value={newTag}
          onChange={(e) => setNewTag(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), onAddTag())}
          className="h-8 text-[12px] bg-[#F3EBDD]/70 border-emerald-ink/15 text-emerald-ink flex-1"
        />
        <TagDatalist id="tag-suggestions" tags={tags} availableTags={availableTags} />
        <Button
          variant="outline"
          size="sm"
          onClick={onAddTag}
          className="h-8 px-3 border-emerald-ink/15 text-emerald-ink"
        >
          <Plus className="w-3.5 h-3.5 mr-1" aria-hidden="true" /> Add
        </Button>
      </div>
    </div>
  );
}
