/**
 * Saved statement passwords for this instrument.
 */
import { KeyRound } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import type { useInstrumentForm } from '@/components/instruments/useInstrumentForm';

type Passwords = ReturnType<typeof useInstrumentForm>['instrumentPasswords'];

/** Saved statement passwords for this instrument. */
export default function SavedPasswordsCard({
  passwords,
  onForget,
  isForgetting,
}: {
  passwords: Passwords;
  onForget: (id: string) => void;
  isForgetting: boolean;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Saved Statement Password</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {passwords.map((p) => (
          <div
            key={p.id}
            className="flex items-center justify-between p-2 rounded-md border border-border"
          >
            <div className="flex items-center gap-2">
              <KeyRound className="w-4 h-4 text-muted-foreground" aria-hidden="true" />
              <span className="text-sm">
                Used {p.success_count} time{p.success_count === 1 ? '' : 's'}
              </span>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => onForget(p.id)}
              disabled={isForgetting}
            >
              Forget Password
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
