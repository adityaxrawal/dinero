import { useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { TransactionAuditRows } from '@/components/transactions/TransactionFields';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';

type Tx = NonNullable<ReturnType<typeof useTransactionForm>['tx']>;

export default function AuditCard({ tx }: { tx: Tx }) {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs overflow-hidden">
      <CardHeader
        className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03] flex flex-row items-center justify-between cursor-pointer select-none hover:bg-[#064E3B]/[0.06] transition-colors"
        onClick={() => setIsOpen((prev) => !prev)}
      >
        <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
          Audit &amp; Technical Specs
        </CardTitle>
        <span className="text-[#064E3B]/60">
          {isOpen ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </span>
      </CardHeader>
      {isOpen && (
        <CardContent className="p-0 animate-in fade-in duration-150">
          <TransactionAuditRows tx={tx} />
        </CardContent>
      )}
    </Card>
  );
}
