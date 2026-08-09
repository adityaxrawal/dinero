import { Cpu } from 'lucide-react';

export default function CleanupEmptyState() {
  return (
    <div className="p-5 rounded-xl border border-dashed border-[#064E3B]/15 bg-[#F8E7C9]/40 flex items-start gap-2.5">
      <Cpu className="w-4 h-4 mt-0.5 shrink-0 text-[#064E3B]/50" />
      <p className="text-[13px] text-[#064E3B]/65 leading-relaxed">
        Nothing to clean up. Every merchant name currently scores above the confidence threshold, so
        there is nothing worth spending inference on.
      </p>
    </div>
  );
}
