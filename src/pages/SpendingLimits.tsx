/**
 * Budget and spending-limit configuration.
 */
import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';

/** Budget and spending-limit configuration. */
export default function SpendingLimits() {
  const navigate = useNavigate();

  useEffect(() => {
    navigate('/settings?section=budgets', { replace: true });
  }, [navigate]);

  return null;
}
