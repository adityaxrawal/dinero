import { KeyRound, ShieldCheck } from 'lucide-react';

interface VaultEntry {
  id: string;
  success_count: number;
}

export default function PasswordVaultList({
  passwords,
  onForget,
  isForgetting,
}: {
  passwords: VaultEntry[];
  onForget: (id: string) => void;
  isForgetting: boolean;
}) {
  return (
    <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-2 shadow-xs">
      <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 flex items-center gap-1.5">
        <ShieldCheck className="w-3.5 h-3.5 text-[#064E3B]" /> Saved Statement Passwords
      </h4>
      {passwords.map((p) => (
        <div
          key={p.id}
          className="flex items-center justify-between p-2.5 rounded-xl border border-[#064E3B]/10 bg-[#F3EBDD]/70"
        >
          <div className="flex items-center gap-2">
            <KeyRound className="w-3.5 h-3.5 text-[#064E3B]" />
            <span className="text-[13px] text-[#064E3B] font-semibold">
              Password vault entry (Used {p.success_count}x)
            </span>
          </div>
          <button
            type="button"
            onClick={() => onForget(p.id)}
            disabled={isForgetting}
            className="text-xs font-semibold px-2.5 py-1 rounded-lg transition-colors hover:bg-red-50 text-red-600 cursor-pointer"
          >
            Forget
          </button>
        </div>
      ))}
    </div>
  );
}
