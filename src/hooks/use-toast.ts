import * as React from 'react';

import type { ToastActionElement, ToastProps } from '@/components/ui/toast';

/**
 * Toast notification store, following the standard shadcn pattern.
 *
 * Deliberately a module-level singleton rather than React Context, which is what
 * makes `toast()` callable from anywhere -- including outside the component
 * tree, where much of this app's error handling lives (IPC failures, event
 * handlers, store subscriptions). A Context-based implementation would be
 * unreachable from those call sites.
 *
 * State lives in a module variable, mutated through a reducer, with mounted
 * components subscribing via a listener array. Dismissal is a two-stage process:
 * DISMISS_TOAST marks a toast closed so it can animate out, and REMOVE_TOAST
 * deletes it once that animation has finished. Timers for both stages are held
 * in module-level maps keyed by toast id.
 */

// At most three visible at once; older toasts are pushed out rather than
// stacking indefinitely.
const TOAST_LIMIT = 3;
// How long a toast remains before dismissing itself.
const TOAST_AUTO_DISMISS_DELAY = 6000;
// Grace period between dismissal and removal, matching the exit animation.
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

/**
 * Monotonic id for each toast.
 *
 * A counter rather than a timestamp, so two toasts raised in the same
 * millisecond cannot collide. Wraps at MAX_SAFE_INTEGER, which is unreachable
 * in practice but keeps the arithmetic exact.
 */
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

// Pending timers, keyed by toast id. Two maps because the removal and
// auto-dismiss stages run on independent schedules and must be cancellable
// separately.
const toastTimeouts = new Map<string, ReturnType<typeof setTimeout>>();
const autoDismissTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

/**
 * Schedule final removal of a dismissed toast, after its exit animation.
 *
 * The early return makes this idempotent -- dismissing an already-dismissed
 * toast must not queue a second removal or restart the delay.
 */
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

/**
 * Start the countdown after which a toast dismisses itself.
 *
 * Idempotent for the same reason: re-rendering must not extend a toast's life
 * by restarting its timer.
 */
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

/**
 * Pure state transitions for the toast queue.
 *
 * DISMISS_TOAST is the one case with a side effect -- it schedules removal --
 * which is a documented deviation from purity in this well-known pattern; the
 * animation timing has to be driven from somewhere.
 */
const reducer = (state: State, action: Action): State => {
  switch (action.type) {
    // Newest first, truncated to the limit. New toasts push out old ones.
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

    // Stage one of dismissal: flag the toast closed so it animates out, and
    // queue the actual removal. An absent toastId means "dismiss everything".
    case 'DISMISS_TOAST': {
      const { toastId } = action;

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
    // Stage two: the toast leaves state entirely once its animation is done.
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

// Mounted components that want to re-render when the queue changes.
const listeners: Array<(state: State) => void> = [];

// The store itself. Module-level, so it survives every unmount and is reachable
// from non-React code.
let memoryState: State = { toasts: [] };

/** Apply an action and notify every subscriber of the new state. */
function dispatch(action: Action) {
  memoryState = reducer(memoryState, action);
  listeners.forEach((listener) => {
    listener(memoryState);
  });
}

type Toast = Omit<ToasterToast, 'id'>;

/**
 * Raise a toast. The primary entry point, callable from anywhere.
 *
 * Returns handles for updating or dismissing the toast after the fact, which is
 * what allows a long-running operation to post one toast and mutate it as
 * progress is made rather than emitting several.
 */
function toast({ ...props }: Toast) {
  const id = genId();

  /** Replaces this toast's content in place, for a long-running operation. */
  const update = (props: ToasterToast) =>
    dispatch({
      type: 'UPDATE_TOAST',
      toast: { ...props, id },
    });
  /** Dismisses this toast. */
  const dismiss = () => dispatch({ type: 'DISMISS_TOAST', toastId: id });

  dispatch({
    type: 'ADD_TOAST',
    toast: {
      ...props,
      id,
      open: true,
      // Bridges the Radix primitive's own close affordances (the X button,
      // Escape, a swipe) back into this store's dismissal path.
      onOpenChange: (open) => {
        if (!open) dismiss();
      },
    },
  });
  // Started after the toast is in state, so its lifetime begins when visible.
  scheduleAutoDismiss(id);

  return {
    id: id,
    dismiss,
    update,
  };
}

/**
 * React binding: subscribe a component to the toast queue.
 *
 * Needed only by components that render toasts (the viewport) or that want to
 * dismiss them. Raising a toast requires no hook -- call `toast()` directly.
 *
 * Initial state is read from the module store rather than starting empty, so a
 * component mounting while toasts are already visible renders them at once.
 */
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

export { useToast, toast };
