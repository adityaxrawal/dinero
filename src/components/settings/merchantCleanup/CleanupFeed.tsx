import { ArrowRight, CheckCircle2, MinusCircle } from 'lucide-react';
import type { FeedEntry } from './format';

function FeedRow({ entry }: { entry: FeedEntry }) {
  return (
    <li className="flex items-center gap-2 text-[12px] animate-fade-in">
      {entry.after ? (
        <CheckCircle2 className="w-3.5 h-3.5 shrink-0 text-emerald-600" />
      ) : (
        <MinusCircle className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/30" />
      )}
      <span className="font-mono text-[#064E3B]/55 truncate max-w-[40%]">{entry.before}</span>
      <ArrowRight className="w-3 h-3 shrink-0 text-[#064E3B]/30" />
      {entry.after ? (
        <>
          <span className="font-semibold text-[#064E3B] truncate">{entry.after}</span>
          {entry.category && (
            <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07] text-[#064E3B]/70 shrink-0">
              {entry.category}
            </span>
          )}
        </>
      ) : (
        <span className="text-[#064E3B]/45 italic">
          left alone — no email kept, or the answer did not check out
        </span>
      )}
    </li>
  );
}

/** The model's answers as they land — the proof a long, quiet run is working. */
export default function CleanupFeed({ feed }: { feed: FeedEntry[] }) {
  if (feed.length === 0) return null;

  return (
    <ul className="mt-3 pt-3 border-t border-[#064E3B]/10 space-y-1.5">
      {feed.map((f) => (
        <FeedRow key={f.key} entry={f} />
      ))}
    </ul>
  );
}
