import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { ShieldOff } from 'lucide-react';
import { NETWORK_DISCLOSURE_TABLE } from '@/constants/privacy';

interface NetworkDisclosureScreenProps {
  onContinue: () => void;
}

// TASK-FE-004 (Doc 30): must be shown before any network-enabled feature is
// reachable, and requires an explicit "Continue" click — never a timed
// auto-advance. Renders Document 01 §10.4's disclosure table verbatim (via
// NETWORK_DISCLOSURE_TABLE, the same source of truth the Gmail consent
// screen and Settings → Privacy → Network Activity render from).
export default function NetworkDisclosureScreen({ onContinue }: NetworkDisclosureScreenProps) {
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-2xl shadow-2xl">
        <CardHeader>
          <CardTitle className="text-2xl">How Dinero Uses the Network</CardTitle>
          <CardDescription className="mt-2">
            Financial data never leaves your Mac. The app makes network calls only for the purposes
            listed below, each strictly limited in scope.
          </CardDescription>
        </CardHeader>

        <CardContent>
          <div
            className="border border-border rounded-lg overflow-hidden"
            role="table"
            aria-label="Network communication disclosure"
          >
            <div
              className="grid grid-cols-[1fr_2fr_1fr] gap-3 bg-secondary/50 px-4 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground"
              role="row"
            >
              <span role="columnheader">Destination</span>
              <span role="columnheader">What Is Sent</span>
              <span role="columnheader">When</span>
            </div>
            {NETWORK_DISCLOSURE_TABLE.map((row) => (
              <div
                key={row.destination}
                className="grid grid-cols-[1fr_2fr_1fr] gap-3 px-4 py-3 text-sm border-t border-border items-start"
                role="row"
              >
                <span className="font-medium" role="cell">
                  {row.destination}
                </span>
                <span className="text-muted-foreground text-xs" role="cell">
                  {row.dataSent}
                </span>
                <span className="text-muted-foreground text-xs" role="cell">
                  {row.when}
                </span>
              </div>
            ))}
          </div>

          <div className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
            <ShieldOff className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
            <span>
              Financial data — transactions, balances, statements — is never sent for any of the
              above. No third-party analytics or crash-reporting services are used.
            </span>
          </div>
        </CardContent>

        <CardFooter className="justify-end">
          <Button
            onClick={onContinue}
            variant="accent"
            aria-label="Continue past the network disclosure"
          >
            Continue
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}
