import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';

/**
 * Redirection page. Spending limits are now managed in Settings > Budgets.
 */
export default function SpendingLimits() {
  const navigate = useNavigate();

  useEffect(() => {
    navigate('/settings?section=budgets', { replace: true });
  }, [navigate]);

  return null;
}
