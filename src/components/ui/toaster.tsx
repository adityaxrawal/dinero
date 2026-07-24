'use client';

import { useToast } from '@/hooks/use-toast';
import { Link } from 'react-router-dom';
import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
  ToastAction,
} from '@/components/ui/toast';

export function Toaster() {
  const { toasts } = useToast();

  return (
    <ToastProvider>
      {toasts.map(function ({ id, title, description, action, actionTo, actionLabel, ...props }) {
        return (
          <Toast key={id} {...props}>
            <div className="grid gap-1">
              {title && <ToastTitle>{title}</ToastTitle>}
              {description && <ToastDescription>{description}</ToastDescription>}
            </div>
            {actionTo && actionLabel ? (
              <ToastAction altText={actionLabel} asChild>
                <Link to={actionTo}>{actionLabel}</Link>
              </ToastAction>
            ) : (
              action
            )}
            <ToastClose />
          </Toast>
        );
      })}
      <ToastViewport />
    </ToastProvider>
  );
}
