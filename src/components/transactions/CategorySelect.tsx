/**
 * Category chooser for a transaction.
 */
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

interface Category {
  id: string;
  name: string;
  color: string | null;
}

interface CategorySelectProps {
  categoryId: string;
  onChange: (value: string) => void;
  categories: Category[];
  id?: string;
  triggerClassName?: string;
}

/** Category chooser for a transaction. */
export function CategorySelect({ categoryId, onChange, categories, id, triggerClassName }: CategorySelectProps) {
  return (
    <Select value={categoryId} onValueChange={onChange}>
      <SelectTrigger id={id} className={triggerClassName || "w-[180px]"}>
        <SelectValue placeholder="Select category" />
      </SelectTrigger>
      <SelectContent>
        {categories.map((c) => (
          <SelectItem key={c.id} value={c.id}>
            <span className="flex items-center gap-2">
              <span
                className="w-2 h-2 rounded-full shrink-0"
                style={{ background: c.color ?? '#064E3B' }}
                aria-hidden="true"
              />
              {c.name}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
