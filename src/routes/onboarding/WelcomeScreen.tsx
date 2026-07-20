import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { ShieldCheck, Zap, LayoutGrid, RefreshCw } from 'lucide-react';

interface WelcomeScreenProps {
  onContinue: () => void;
}

// TASK-FE-004 (Doc 30): app introduction and value proposition, first screen
// of the onboarding flow. Grounded in Document 01 §6.1/§6.2's vision
// statement and core value propositions — not freely invented marketing copy.
const VALUE_PROPS = [
  {
    icon: Zap,
    title: 'Zero manual data entry',
    description: 'Transactions are captured automatically from Gmail, categorized, and deduplicated — no spreadsheets.',
  },
  {
    icon: LayoutGrid,
    title: 'Every account, one dashboard',
    description: 'All your credit cards, debit cards, and bank accounts in a single, unified view, organized by issuer.',
  },
  {
    icon: ShieldCheck,
    title: 'Complete privacy',
    description: 'The optional local LLM runs entirely on your Mac. No financial data ever leaves your device.',
  },
  {
    icon: RefreshCw,
    title: 'Self-improving accuracy',
    description: 'The extraction engine learns from your corrections locally — repeated mistakes disappear over time.',
  },
];

export default function WelcomeScreen({ onContinue }: WelcomeScreenProps) {
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-lg shadow-2xl">
        <CardHeader>
          <CardTitle className="text-2xl">Welcome to Dinero</CardTitle>
          <CardDescription className="mt-2">
            The most trusted personal finance tool for privacy-conscious Mac users — the only tool that
            knows your complete financial picture without ever leaving your device.
          </CardDescription>
        </CardHeader>

        <CardContent>
          <ul className="space-y-4">
            {VALUE_PROPS.map(({ icon: Icon, title, description }) => (
              <li key={title} className="flex items-start gap-3">
                <div className="mt-0.5 w-8 h-8 shrink-0 rounded-lg bg-[#064E3B]/10 flex items-center justify-center">
                  <Icon className="w-4 h-4 text-[#064E3B]" aria-hidden="true" />
                </div>
                <div>
                  <p className="text-sm font-medium">{title}</p>
                  <p className="text-xs text-muted-foreground mt-0.5">{description}</p>
                </div>
              </li>
            ))}
          </ul>
        </CardContent>

        <CardFooter className="justify-end">
          <Button onClick={onContinue} variant="accent" aria-label="Get started with Dinero setup">
            Get Started
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}
