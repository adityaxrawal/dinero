/**
 * Appearance settings.
 */
import { Palette } from 'lucide-react';
import { Button } from '@/components/ui/button';
import MenuBarExtraSettings from '@/components/settings/MenuBarExtraSettings';
import SectionHeading from '@/components/settings/SectionHeading';

/** Appearance settings. */
export default function AppearanceSection() {
  return (
    <div className="animate-in fade-in duration-300 space-y-12">
      <section>
        <SectionHeading
          icon={Palette}
          title="Appearance"
          description="Dinero currently ships dark-mode only. Light mode is planned for a future release."
        />
        <div className="flex gap-4 max-w-sm p-1 rounded-xl bg-emerald-ink/5 border border-emerald-ink/10">
          <Button
            className="flex-1 cursor-default h-9 font-semibold bg-emerald-ink text-champagne shadow-sm rounded-lg"
            disabled
          >
            Dark Mode (Active)
          </Button>
          <Button
            variant="ghost"
            className="flex-1 cursor-default h-9 font-semibold text-emerald-ink/60 hover:text-emerald-ink/60 hover:bg-transparent"
            disabled
          >
            Light Mode
          </Button>
        </div>
      </section>

      <div className="h-px w-full bg-emerald-ink/10" />

      <section>
        <MenuBarExtraSettings />
      </section>
    </div>
  );
}
