import { AlertTriangle } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { InstrumentRecord } from '@/lib/ipc';
import { instrumentIcon } from './instrumentTypes';

interface InstrumentCardProps {
  inst: InstrumentRecord;
}

/** TASK-FE-011 (Doc 30): a grid card for InstrumentsList, linking to InstrumentDetail. */
export default function InstrumentCard({ inst }: InstrumentCardProps) {
  const navigate = useNavigate();
  const isNegative = (inst.current_balance ?? 0) < 0;

  return (
    <Card
      className="cursor-pointer hover:border-[#064E3B]/40 hover:shadow-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onClick={() => navigate(`/instruments/${inst.id}`)}
      tabIndex={0}
      role="button"
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && navigate(`/instruments/${inst.id}`)}
      aria-label={`${inst.issuer_name} ${inst.masked_identifier}`}
    >
      <CardContent className="p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="p-2 bg-muted rounded-full">
            {instrumentIcon(inst.instrument_type)}
          </div>
          <Badge variant={inst.status === 'active' ? 'default' : 'secondary'} className="text-[10px]">
            {inst.status}
          </Badge>
        </div>
        <div>
          <p className="font-medium">{inst.issuer_name}</p>
          <p className="text-sm text-muted-foreground">{inst.full_identifier || inst.masked_identifier}</p>
        </div>
        <div className="flex items-center justify-between pt-2 border-t border-border">
          <span className="text-xs text-muted-foreground">Balance</span>
          <div className="flex items-center gap-1.5">
            {isNegative && (
              <AlertTriangle className="w-3.5 h-3.5 text-red-700" aria-label="Negative balance detected" />
            )}
            <span className={isNegative ? 'text-red-700 font-semibold text-sm' : 'font-semibold text-sm'}>
              {inst.current_balance != null ? `₹${inst.current_balance.toFixed(2)}` : '—'}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
