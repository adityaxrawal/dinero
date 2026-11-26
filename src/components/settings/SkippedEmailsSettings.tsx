/**
 * Every email the pipeline skipped in the last 30 days, with the reason, so a
 * wrongly rejected bank sender can be spotted and trusted (owner decision Q-8,
 * audit F5-3). Trusted senders are review-only: their mail is pre-filled in the
 * Review Inbox, never booked automatically.
 */
import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { MailX } from 'lucide-react';
import SectionHeading from '@/components/settings/SectionHeading';
import { Button } from '@/components/ui/button';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';

const SKIPPED_KEY = ['skippedEmails'] as const;

/** Plain-language reason for a skip code. */
function reasonLabel(reason: string): string {
  if (reason.startsWith('Unlisted subdomain')) return 'Unlisted address for a known bank';
  if (reason.startsWith('Suspicious domain')) return 'Look-alike of a bank address';
  if (reason.startsWith('Mismatched display name')) return "Name doesn't match the sender address";
  if (reason.startsWith('Typo-squatted')) return 'Look-alike of a bank address';
  if (reason === 'Unknown sender domain') return 'Sender not on the bank list';
  if (reason === 'gate1_verified_noise') return "This sender doesn't send transactions";
  const kind = reason.match(/^gate2a?_reject_(\w+)$/)?.[1];
  const KINDS: Record<string, string> = {
    Otp: 'One-time password',
    Marketing: 'Marketing',
    Reminder: 'Payment reminder',
    Kyc: 'KYC / account notice',
    Noise: 'Not a transaction',
    Unknown: 'Not a transaction',
  };
  return (kind && KINDS[kind]) || reason;
}

export default function SkippedEmailsSettings() {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const { data: skipped = [], isError, refetch } = useQuery({
    queryKey: SKIPPED_KEY,
    queryFn: API.skippedEmails.list,
  });
  const [trusting, setTrusting] = useState<string | null>(null);

  const trust = (domain: string) => {
    setTrusting(domain);
    API.skippedEmails
      .trustSender(domain)
      .then((n) => {
        queryClient.invalidateQueries({ queryKey: SKIPPED_KEY });
        toast({
          title: `Trusted ${domain}`,
          description: `${n} email${n === 1 ? '' : 's'} will be read on the next scan and sent to the Review Inbox for you to check.`,
        });
      })
      .catch((err) => toast({ variant: 'destructive', ...getErrorToast(err) }))
      .finally(() => setTrusting(null));
  };

  const trustedOffered = new Set<string>();

  return (
    <section className="mt-10" aria-label="Skipped emails">
      <SectionHeading
        icon={MailX}
        title="Skipped emails"
        description="Emails from the last 30 days that weren't treated as transactions, and why. If a real bank email was rejected by the sender check, trust the sender."
      />
      {isError ? (
        <p role="alert" className="text-[12px] text-emerald-ink/70">
          Couldn't load skipped emails.{' '}
          <button type="button" className="underline" onClick={() => refetch()}>
            Try again
          </button>
        </p>
      ) : skipped.length === 0 ? (
        <p className="text-[12px] text-emerald-ink/60">Nothing skipped in the last 30 days.</p>
      ) : (
        <ul className="max-h-[420px] overflow-y-auto divide-y divide-emerald-ink/10 rounded-xl border border-emerald-ink/10">
          {skipped.map((e) => {
            const domain = e.sender_domain ?? '';
            const offerTrust = e.gate === 'sender' && domain && !trustedOffered.has(domain);
            if (offerTrust) trustedOffered.add(domain);
            return (
              <li key={e.message_id} className="flex items-start gap-3 p-3 text-[12px] text-emerald-ink">
                <div className="min-w-0 flex-1">
                  <p className="truncate font-medium">{e.subject || '(no subject)'}</p>
                  <p className="truncate text-emerald-ink/60">
                    {domain || e.bank_name || 'Unknown sender'} · {reasonLabel(e.reason)}
                  </p>
                </div>
                {offerTrust && (
                  <Button
                    size="sm"
                    variant="outline"
                    className="shrink-0 h-7 text-[11px]"
                    disabled={trusting === domain}
                    onClick={() => trust(domain)}
                  >
                    Trust this sender
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
