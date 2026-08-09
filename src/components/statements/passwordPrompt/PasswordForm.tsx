import { useState } from 'react';
import { Lock, AlertTriangle, Eye, EyeOff } from 'lucide-react';
import { DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import type { useStatementPasswordPrompt } from './useStatementPasswordPrompt';

type Prompt = ReturnType<typeof useStatementPasswordPrompt>;

export default function PasswordForm({ prompt }: { prompt: Prompt }) {
  const [showPassword, setShowPassword] = useState(false);
  const { password, passwordError, isSubmitting, submitPassword } = prompt;

  return (
    <div className="p-8 flex flex-col h-full overflow-y-auto bg-background min-w-0">
      <DialogHeader className="mb-8">
        <DialogTitle
          id="password-dialog-title"
          className="flex items-center gap-2 text-2xl font-semibold tracking-tight text-foreground"
        >
          <Lock className="w-5 h-5 text-destructive" aria-hidden="true" />
          Password Required
        </DialogTitle>
        <DialogDescription
          id="password-dialog-desc"
          className="text-sm mt-2 text-muted-foreground leading-relaxed"
        >
          The uploaded statement is encrypted. Please provide the PDF password to continue
          processing.
        </DialogDescription>
      </DialogHeader>

      <div className="flex-1 flex flex-col justify-center">
        <div className="bg-card p-6 rounded-xl border border-border shadow-sm space-y-4">
          <div className="space-y-3">
            <Label htmlFor="pdf-password" className="text-sm font-medium text-foreground">
              PDF Password
            </Label>
            <div className="relative">
              <Input
                id="pdf-password"
                type={showPassword ? 'text' : 'password'}
                placeholder="Enter PDF password"
                value={password}
                onChange={(e) => {
                  prompt.setPassword(e.target.value);
                  prompt.setPasswordError(null);
                }}
                onKeyDown={(e) => e.key === 'Enter' && submitPassword()}
                aria-invalid={!!passwordError}
                aria-describedby={passwordError ? 'password-error' : undefined}
                autoFocus
                className="h-11 pr-10 border-input focus-visible:ring-ring transition-shadow"
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-sm transition-colors"
                aria-label={showPassword ? 'Hide password' : 'Show password'}
              >
                {showPassword ? (
                  <EyeOff className="h-5 w-5" aria-hidden="true" />
                ) : (
                  <Eye className="h-5 w-5" aria-hidden="true" />
                )}
              </button>
            </div>
            {passwordError && (
              <p
                id="password-error"
                role="alert"
                className="text-sm text-destructive flex items-center gap-1.5 mt-2 animate-in fade-in slide-in-from-top-1"
              >
                <AlertTriangle className="w-4 h-4" aria-hidden="true" />
                {passwordError}
              </p>
            )}
          </div>

          <DialogFooter className="pt-2 grid grid-cols-2 gap-3 sm:space-x-0">
            <Button
              variant="outline"
              onClick={prompt.close}
              aria-label="Cancel password entry"
              className="h-11 w-full border-input text-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
            >
              Cancel
            </Button>
            <Button
              onClick={submitPassword}
              disabled={!password.trim() || isSubmitting}
              aria-label="Submit PDF password"
              className="h-11 w-full bg-primary hover:bg-primary/90 text-primary-foreground shadow-sm transition-all"
            >
              {isSubmitting ? 'Unlocking…' : 'Unlock & Parse'}
            </Button>
          </DialogFooter>
        </div>
      </div>
    </div>
  );
}
