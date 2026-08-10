// Covers the Onboarding page shell -- which step renders, when the footer
// is suppressed, and the step->step wiring. The individual screens have
// their own specs; none of them render this container.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Onboarding from '@/pages/Onboarding';

const navigate = vi.fn();
const setStep = vi.fn();
let step = 1;
let statementPref = 'auto';

vi.mock('react-router-dom', () => ({ useNavigate: () => navigate }));
vi.mock('@/pages/onboarding/useOnboardingPreferences', () => ({
  useOnboardingPreferences: () => ({
    statementPref,
    validateLimit: vi.fn(),
    persist: vi.fn(),
  }),
}));
vi.mock('@/pages/onboarding/useOnboardingSteps', () => ({
  TOTAL_STEPS: 4,
  useOnboardingSteps: () => ({
    step,
    setStep,
    loading: false,
    oauthError: null,
    handleBack: vi.fn(),
    handleNext: vi.fn(),
    handleConnectGmail: vi.fn(),
    handleSkipGmail: vi.fn(),
  }),
}));

vi.mock('@/pages/onboarding/PreferencesStep', () => ({
  default: () => <div data-testid="preferences" />,
}));
vi.mock('@/pages/onboarding/OnboardingFooter', () => ({ default: () => <div data-testid="footer" /> }));
vi.mock('@/routes/onboarding/GmailConsentScreen', () => ({
  default: ({ showSkip }: { showSkip: boolean }) => (
    <div data-testid="gmail">{showSkip ? 'skippable' : 'required'}</div>
  ),
}));
vi.mock('@/routes/onboarding/HistoricalScanScreen', () => ({
  default: ({ onDone }: { onDone: () => void }) => (
    <button type="button" data-testid="scan" onClick={onDone}>
      done
    </button>
  ),
}));
vi.mock('@/routes/onboarding/LicenseActivationScreen', () => ({
  default: ({ onContinue }: { onContinue: () => void }) => (
    <button type="button" data-testid="license" onClick={onContinue}>
      continue
    </button>
  ),
}));

beforeEach(() => {
  vi.clearAllMocks();
  step = 1;
  statementPref = 'auto';
});

describe('Onboarding step rendering', () => {
  it.each([
    [1, 'preferences'],
    [2, 'gmail'],
    [3, 'scan'],
    [4, 'license'],
  ])('renders only the step %i screen', (current, testId) => {
    step = current;
    render(<Onboarding />);
    expect(screen.getByTestId(testId)).toBeInTheDocument();
    for (const other of ['preferences', 'gmail', 'scan', 'license'].filter((t) => t !== testId)) {
      expect(screen.queryByTestId(other)).not.toBeInTheDocument();
    }
  });

  it('tracks progress on the progressbar', () => {
    step = 3;
    render(<Onboarding />);
    const bar = screen.getByRole('progressbar', { name: 'Onboarding progress' });
    expect(bar).toHaveAttribute('aria-valuenow', '3');
    expect(bar).toHaveAttribute('aria-valuemax', '4');
    expect(screen.getByLabelText('Step 3 of 4')).toBeInTheDocument();
  });
});

describe('Onboarding footer visibility', () => {
  it.each([1, 2])('shows the footer on step %i', (current) => {
    step = current;
    render(<Onboarding />);
    expect(screen.getByTestId('footer')).toBeInTheDocument();
  });

  it.each([3, 4])('hides the footer on step %i, where the screen owns its actions', (current) => {
    step = current;
    render(<Onboarding />);
    expect(screen.queryByTestId('footer')).not.toBeInTheDocument();
  });
});

describe('Onboarding wiring', () => {
  it('offers a Gmail skip only when statements are handled manually', () => {
    step = 2;
    const { unmount } = render(<Onboarding />);
    expect(screen.getByTestId('gmail')).toHaveTextContent('required');
    unmount();

    statementPref = 'manual';
    render(<Onboarding />);
    expect(screen.getByTestId('gmail')).toHaveTextContent('skippable');
  });

  it('advances to the license step once the historical scan finishes', () => {
    step = 3;
    render(<Onboarding />);
    fireEvent.click(screen.getByTestId('scan'));
    expect(setStep).toHaveBeenCalledWith(4);
  });

  it('leaves onboarding for the dashboard on the final step', () => {
    step = 4;
    render(<Onboarding />);
    fireEvent.click(screen.getByTestId('license'));
    expect(navigate).toHaveBeenCalledWith('/');
  });
});
