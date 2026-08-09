import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { formatCustomDate } from '@/lib/formatCustomDate';
import type { useInstrumentForm } from '@/components/instruments/useInstrumentForm';

type Statements = ReturnType<typeof useInstrumentForm>['instrumentStatements'];

export default function StatementHistoryCard({ statements }: { statements: Statements }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Statement History</CardTitle>
      </CardHeader>
      <CardContent>
        {statements.length === 0 ? (
          <p className="text-sm text-muted-foreground">No statements for this instrument yet.</p>
        ) : (
          <ul className="space-y-2">
            {statements.map((s) => (
              <li key={s.id} className="flex items-center justify-between text-sm">
                <span>{s.file_name}</span>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-muted-foreground">{formatCustomDate(s.date)}</span>
                  <Badge
                    variant={s.status === 'PROCESSED' ? 'default' : 'secondary'}
                    className="text-[10px]"
                  >
                    {s.status}
                  </Badge>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
