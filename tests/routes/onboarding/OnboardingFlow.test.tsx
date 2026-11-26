// Review P4: background network calls wait for the disclosure acknowledgement,
// so the flow advances only once it is stored.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import OnboardingFlow from '@/routes/onboarding/OnboardingFlow';

const navigate = vi.fn();
let locationState: unknown = null;
const recordConsentEvent = vi.fn();

vi.mock('react-router-dom', () => ({
  useNavigate: () => navigate,
  useLocation: () => ({ state: locationState }),
}));
vi.mock('@/lib/ipc', () => ({
  API: { privacy: { recordConsentEvent: (...a: unknown[]) => recordConsentEvent(...a) } },
}));
vi.mock('@/routes/onboarding/WelcomeScreen', () => ({
  default: ({ onContinue }: { onContinue: () => void }) => (
    <button onClick={onContinue}>welcome</button>
  ),
}));
vi.mock('@/routes/onboarding/SetupSteps', () => ({ default: () => <div data-testid="setup" /> }));

const continueDisclosure = () =>
  fireEvent.click(screen.getByRole('button', { name: /continue past the network disclosure/i }));

describe('OnboardingFlow disclosure', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    locationState = null;
  });

  it('stays on the disclosure with a message when the record fails', async () => {
    recordConsentEvent.mockRejectedValue(new Error('db locked'));
    render(<OnboardingFlow />);
    fireEvent.click(screen.getByText('welcome'));
    continueDisclosure();
    expect(await screen.findByRole('alert')).toHaveTextContent('db locked');
    expect(screen.queryByTestId('setup')).toBeNull();
  });

  it('moves to setup once the record is stored', async () => {
    recordConsentEvent.mockResolvedValue(undefined);
    render(<OnboardingFlow />);
    fireEvent.click(screen.getByText('welcome'));
    continueDisclosure();
    expect(await screen.findByTestId('setup')).toBeInTheDocument();
  });

  it('consent-only: an onboarded install sees just the disclosure, then returns to the app', async () => {
    locationState = { consentOnly: true };
    recordConsentEvent.mockResolvedValue(undefined);
    render(<OnboardingFlow />);
    continueDisclosure();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith('/', { replace: true }));
  });
});
