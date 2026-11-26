/**
 * Create, rename and delete your own categories, and delete tags. The backend
 * commands existed with no UI (audit F21-1, owner decision Q-11).
 */
import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Tags, Trash2 } from 'lucide-react';
import SectionHeading from '@/components/settings/SectionHeading';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useTagsList } from '@/hooks/queries/useTagsList';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';
import { getErrorToast } from '@/lib/errorMapping';
import { queryKeys } from '@/lib/queryKeys';

export default function CategoriesAndTagsSettings() {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const { data: categories = [] } = useCategoriesList();
  const { data: tags = [] } = useTagsList();
  const [newName, setNewName] = useState('');
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null);

  const refresh = () => {
    queryClient.invalidateQueries({ queryKey: queryKeys.categories.all() });
    queryClient.invalidateQueries({ queryKey: queryKeys.tags.all() });
    queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
  };
  const run = (action: () => Promise<unknown>, done: string) =>
    action()
      .then(() => {
        refresh();
        toast({ title: done });
      })
      .catch((err) => toast({ variant: 'destructive', ...getErrorToast(err) }));

  const custom = categories.filter((c) => c.source_type !== 'system' && !c.is_deleted);

  const add = () => {
    const name = newName.trim();
    if (!name) return;
    run(() => API.categories.create(name), `Added "${name}"`).then(() => setNewName(''));
  };
  const saveRename = () => {
    if (!editing?.name.trim()) return;
    const { id, name } = editing;
    run(() => API.categories.rename(id, name.trim()), 'Category renamed').then(() => setEditing(null));
  };
  const removeCategory = async (id: string, name: string) => {
    const ok = await confirmAction(
      `Delete "${name}"? Its transactions move to "Others".`,
      'Delete category'
    );
    if (ok) run(() => API.categories.delete(id), `Deleted "${name}"`);
  };
  const removeTag = async (id: string, name: string) => {
    const ok = await confirmAction(`Delete the tag "${name}" from every transaction?`, 'Delete tag');
    if (ok) run(() => API.tags.delete(id), `Deleted tag "${name}"`);
  };

  return (
    <section className="mt-10" aria-label="Categories and tags">
      <SectionHeading
        icon={Tags}
        title="Categories & tags"
        description="Add your own categories, rename or delete them, and clean up tags. Built-in categories can't be changed."
      />

      <div className="space-y-6">
        <div className="flex gap-2">
          <Input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && add()}
            placeholder="New category name"
            aria-label="New category name"
            className="h-9 text-[13px]"
          />
          <Button onClick={add} disabled={!newName.trim()} className="h-9 text-[13px]">
            Add category
          </Button>
        </div>

        <ul className="divide-y divide-emerald-ink/10 rounded-xl border border-emerald-ink/10">
          {custom.length === 0 && (
            <li className="p-3 text-[12px] text-emerald-ink/60">No custom categories yet.</li>
          )}
          {custom.map((c) => (
            <li key={c.id} className="flex items-center gap-2 p-3 text-[13px] text-emerald-ink">
              {editing?.id === c.id ? (
                <>
                  <Input
                    value={editing.name}
                    onChange={(e) => setEditing({ id: c.id, name: e.target.value })}
                    onKeyDown={(e) => e.key === 'Enter' && saveRename()}
                    aria-label={`New name for ${c.name}`}
                    className="h-8 text-[13px]"
                    autoFocus
                  />
                  <Button size="sm" onClick={saveRename}>
                    Save
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setEditing(null)}>
                    Cancel
                  </Button>
                </>
              ) : (
                <>
                  <span className="flex-1 truncate">{c.name}</span>
                  <Button size="sm" variant="ghost" onClick={() => setEditing({ id: c.id, name: c.name })}>
                    Rename
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-red-700"
                    onClick={() => removeCategory(c.id, c.name)}
                    aria-label={`Delete category ${c.name}`}
                  >
                    <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                  </Button>
                </>
              )}
            </li>
          ))}
        </ul>

        <div>
          <p className="mb-2 text-[12px] font-medium text-emerald-ink/70">Tags</p>
          {tags.length === 0 ? (
            <p className="text-[12px] text-emerald-ink/60">No tags yet. Add them from a transaction.</p>
          ) : (
            <ul className="flex flex-wrap gap-2">
              {tags.map((t) => (
                <li
                  key={t.id}
                  className="flex items-center gap-1 rounded-full border border-emerald-ink/15 px-2.5 py-1 text-[12px] text-emerald-ink"
                >
                  {t.name}
                  <button
                    type="button"
                    onClick={() => removeTag(t.id, t.name)}
                    aria-label={`Delete tag ${t.name}`}
                    className="text-emerald-ink/50 hover:text-red-700"
                  >
                    <Trash2 className="h-3 w-3" aria-hidden="true" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}
