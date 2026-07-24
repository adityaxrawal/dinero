import * as React from 'react';

import type { ToastActionElement, ToastProps } from '@/components/ui/toast';

// Doc 30 TASK-RT-003 acceptance (`test_toast_queue_caps_at_max_visible`,
// `test_toast_auto_dismisses_after_timeout`): these were left at shadcn's
// stock scaffold values (limit 1, ~16.6-minute removal delay) -- the spec
// calls for a max of 3 simultaneously visible and a 5-8s auto-dismiss.
// `TOAST_REMOVE_DELAY` is a distinct, smaller concern: how long *after*
// `dismiss()` fires (either by timeout or user action) before the toast is
// actually removed from state, giving the exit animation time to play.
const TOAST_LIMIT = 3;
const TOAST_AUTO_DISMISS_DELAY = 6000;
const TOAST_REMOVE_DELAY = 300;

type ToasterToast = ToastProps & {
  id: string;
  title?: React.ReactNode;
  description?: React.ReactNode;
  action?: ToastActionElement;
  actionTo?: string;
  actionLabel?: string;
};

let count = 0;

function genId() {
  count = (count + 1) % Number.MAX_SAFE_INTEGER;
  return count.toString();
}

type Action =
  | {
      type: 'ADD_TOAST';
      toast: ToasterToast;
    }
  | {
      type: 'UPDATE_TOAST';
      toast: Partial<ToasterToast>;
    }
  | {
      type: 'DISMISS_TOAST';
      toastId?: ToasterToast['id'] | undefined;
    }
  | {
      type: 'REMOVE_TOAST';
      toastId?: ToasterToast['id'] | undefined;
    };

interface State {
  toasts: ToasterToast[];
}

const toastTimeouts = new Map<string, ReturnType<typeof setTimeout>>();
const autoDismissTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

const addToRemoveQueue = (toastId: string) => {
  if (toastTimeouts.has(toastId)) {
    return;
  }

  const timeout = setTimeout(() => {
    toastTimeouts.delete(toastId);
    dispatch({
      type: 'REMOVE_TOAST',
      toastId: toastId,
    });
  }, TOAST_REMOVE_DELAY);

  toastTimeouts.set(toastId, timeout);
};

// Doc 30 TASK-RT-003: schedules the actual auto-dismiss -- previously
// nothing ever called `addToRemoveQueue` except a manual `dismiss()`, so a
// toast that nobody clicked stayed on screen indefinitely (bounded only by
// the old 1000000ms `TOAST_REMOVE_DELAY`, which was itself misused as if it
// were this timer).
const scheduleAutoDismiss = (toastId: string) => {
  if (autoDismissTimeouts.has(toastId)) {
    return;
  }
  const timeout = setTimeout(() => {
    autoDismissTimeouts.delete(toastId);
    dispatch({ type: 'DISMISS_TOAST', toastId });
  }, TOAST_AUTO_DISMISS_DELAY);
  autoDismissTimeouts.set(toastId, timeout);
};

const reducer = (state: State, action: Action): State => {
  switch (action.type) {
    case 'ADD_TOAST':
      return {
        ...state,
        toasts: [action.toast, ...state.toasts].slice(0, TOAST_LIMIT),
      };

    case 'UPDATE_TOAST':
      return {
        ...state,
        toasts: state.toasts.map((t) => (t.id === action.toast.id ? { ...t, ...action.toast } : t)),
      };

    case 'DISMISS_TOAST': {
      const { toastId } = action;

      // ! Side effects ! - This could be extracted into a dismissToast() action,
      // but I'll keep it here for simplicity
      if (toastId) {
        addToRemoveQueue(toastId);
      } else {
        state.toasts.forEach((toast) => {
          addToRemoveQueue(toast.id);
        });
      }

      return {
        ...state,
        toasts: state.toasts.map((t) =>
          t.id === toastId || toastId === undefined
            ? {
                ...t,
                open: false,
              }
            : t
        ),
      };
    }
    case 'REMOVE_TOAST':
      if (action.toastId === undefined) {
        return {
          ...state,
          toasts: [],
        };
      }
      return {
        ...state,
        toasts: state.toasts.filter((t) => t.id !== action.toastId),
      };
  }
};

const listeners: Array<(state: State) => void> = [];

let memoryState: State = { toasts: [] };

function dispatch(action: Action) {
  memoryState = reducer(memoryState, action);
  listeners.forEach((listener) => {
    listener(memoryState);
  });
}

type Toast = Omit<ToasterToast, 'id'>;

function toast({ ...props }: Toast) {
  const id = genId();

  const update = (props: ToasterToast) =>
    dispatch({
      type: 'UPDATE_TOAST',
      toast: { ...props, id },
    });
  const dismiss = () => dispatch({ type: 'DISMISS_TOAST', toastId: id });

  dispatch({
    type: 'ADD_TOAST',
    toast: {
      ...props,
      id,
      open: true,
      onOpenChange: (open) => {
        if (!open) dismiss();
      },
    },
  });
  scheduleAutoDismiss(id);

  return {
    id: id,
    dismiss,
    update,
  };
}

function useToast() {
  const [state, setState] = React.useState<State>(memoryState);

  React.useEffect(() => {
    listeners.push(setState);
    return () => {
      const index = listeners.indexOf(setState);
      if (index > -1) {
        listeners.splice(index, 1);
      }
    };
  }, [state]);

  return {
    ...state,
    toast,
    dismiss: (toastId?: string) => dispatch({ type: 'DISMISS_TOAST', toastId }),
  };
}

// TASK-FE-018: `toast` exported directly (not just via `useToast()`) so
// non-component code (`ToastProvider`'s error-toast dispatcher,
// `useIpcInvoke`) can queue a toast without needing a hook — this module's
// state is already a plain module-level singleton, not React context, so
// this requires no new plumbing.
export { useToast, toast };
