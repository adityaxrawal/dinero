import { Loader2, Save, Trash2 } from 'lucide-react';

export default function SaveDeleteBar({
  isSaving,
  isDeleting,
  onSave,
  onDelete,
}: {
  isSaving: boolean;
  isDeleting: boolean;
  onSave: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="flex items-center gap-3 pt-2">
      <button
        type="button"
        onClick={onSave}
        disabled={isSaving}
        className="flex-1 h-10 rounded-xl text-[13px] font-bold flex items-center justify-center gap-2 transition-all bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-sm cursor-pointer"
      >
        {isSaving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
        Save Changes
      </button>
      <button
        type="button"
        onClick={onDelete}
        disabled={isDeleting}
        className="h-10 px-4 rounded-xl text-[13px] font-bold flex items-center justify-center gap-2 transition-colors border border-red-500/30 text-red-700 hover:bg-red-50 cursor-pointer"
      >
        {isDeleting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Trash2 className="w-4 h-4" />}
        Delete Account
      </button>
    </div>
  );
}
