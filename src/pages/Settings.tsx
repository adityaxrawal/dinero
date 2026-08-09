import { useSearchParams } from 'react-router-dom';
import { Palette, CreditCard, Gauge, Shield, Users, Settings as SettingsIcon } from 'lucide-react';
import { PageSidebar } from '@/components/layout/PageSidebar';

import BudgetsSection from './settings/BudgetsSection';
import AccountsSection from './settings/AccountsSection';
import PrivacySection from './settings/PrivacySection';
import LicenseSection from './settings/LicenseSection';
import AdvancedSection from './settings/AdvancedSection';
import AppearanceSection from './settings/AppearanceSection';

type SettingsSection = 'budgets' | 'accounts' | 'privacy' | 'license' | 'advanced' | 'appearance';

const SETTINGS_SECTIONS = [
  { id: 'budgets', label: 'Budgets', icon: Gauge },
  { id: 'accounts', label: 'Connected Accounts', icon: Users },
  { id: 'privacy', label: 'Privacy & Security', icon: Shield },
  { id: 'license', label: 'License & Billing', icon: CreditCard },
  { id: 'advanced', label: 'Advanced', icon: SettingsIcon },
  { id: 'appearance', label: 'Appearance', icon: Palette },
] as const;

const SECTION_CONTENT: Record<SettingsSection, React.ComponentType> = {
  budgets: BudgetsSection,
  accounts: AccountsSection,
  privacy: PrivacySection,
  license: LicenseSection,
  advanced: AdvancedSection,
  appearance: AppearanceSection,
};

export default function Settings() {
  const [searchParams, setSearchParams] = useSearchParams();
  const currentSection = (searchParams.get('section') as SettingsSection) || 'budgets';
  const setSection = (section: SettingsSection) => setSearchParams({ section });

  // An unrecognised ?section= renders an empty pane, as it always has.
  const SectionContent = SECTION_CONTENT[currentSection];

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Navigation (Settings) ─────────────────────────────────── */}
      <PageSidebar
        title="Settings"
        sections={SETTINGS_SECTIONS}
        currentSection={currentSection}
        onSelectSection={setSection}
      />

      {/* ── Column 3: Content Area ────────────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12 text-[#064E3B]">
        <div className="max-w-3xl mx-auto space-y-12">{SectionContent && <SectionContent />}</div>
      </div>
    </div>
  );
}
