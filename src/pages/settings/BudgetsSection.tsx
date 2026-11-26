/**
 * Budget settings section.
 */
import BudgetsSettings from '@/components/settings/BudgetsSettings';
import CategoriesAndTagsSettings from '@/components/settings/CategoriesAndTagsSettings';

/** Budget settings section. */
export default function BudgetsSection() {
  return (
    <div className="animate-in fade-in duration-300">
      <BudgetsSettings />
      <CategoriesAndTagsSettings />
    </div>
  );
}
