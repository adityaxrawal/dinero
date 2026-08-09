export interface BankTheme {
  gradient: string;
  accentColor: string;
  badgeBg: string;
  label: string;
}

/** Matched on a substring of the issuer name, first hit wins. */
const BANK_THEMES: [match: string, theme: BankTheme][] = [
  [
    'idfc',
    {
      gradient: 'from-[#600C12] via-[#7B1113] to-[#3D0609]',
      accentColor: '#FFD700',
      badgeBg: 'bg-[#FFD700]/15 text-[#FFD700] border-[#FFD700]/30',
      label: 'IDFC FIRST',
    },
  ],
  [
    'hdfc',
    {
      gradient: 'from-[#0F3868] via-[#004B8D] to-[#082142]',
      accentColor: '#60A5FA',
      badgeBg: 'bg-blue-400/15 text-blue-300 border-blue-400/30',
      label: 'HDFC Bank',
    },
  ],
  [
    'sbi',
    {
      gradient: 'from-[#1E3A8A] via-[#1D4ED8] to-[#172554]',
      accentColor: '#93C5FD',
      badgeBg: 'bg-sky-400/15 text-sky-200 border-sky-400/30',
      label: 'SBI',
    },
  ],
  [
    'axis',
    {
      gradient: 'from-[#6B0F38] via-[#97144D] to-[#450923]',
      accentColor: '#F472B6',
      badgeBg: 'bg-pink-400/15 text-pink-300 border-pink-400/30',
      label: 'Axis Bank',
    },
  ],
  [
    'jupiter',
    {
      gradient: 'from-[#045C4B] via-[#00897B] to-[#023329]',
      accentColor: '#34D399',
      badgeBg: 'bg-emerald-400/15 text-emerald-300 border-emerald-400/30',
      label: 'Jupiter',
    },
  ],
  [
    'yes',
    {
      gradient: 'from-[#1E3A5F] via-[#0055A5] to-[#0D1F38]',
      accentColor: '#60A5FA',
      badgeBg: 'bg-blue-400/15 text-blue-300 border-blue-400/30',
      label: 'Yes Bank',
    },
  ],
];

/** Default Dinero Dark Emerald, for any issuer without its own palette. */
export function getBankTheme(issuerName: string): BankTheme {
  const name = issuerName.toLowerCase();
  const match = BANK_THEMES.find(([key]) => name.includes(key));
  if (match) return match[1];

  return {
    gradient: 'from-[#064E3B] via-[#043327] to-[#022018]',
    accentColor: '#34D399',
    badgeBg: 'bg-[#F8E7C9]/15 text-[#F8E7C9] border-[#F8E7C9]/20',
    label: issuerName,
  };
}
