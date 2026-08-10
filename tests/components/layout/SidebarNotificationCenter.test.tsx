import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import SidebarNotificationCenter from '@/components/layout/SidebarNotificationCenter';
import { useNotificationStore } from '@/stores/useNotificationStore';

vi.mock('@/lib/tauriRuntime', () => ({
  isTauriRuntime: () => false,
}));

vi.mock('@/lib/ipc', () => ({
  API: {
    backgroundTasks: {
      getActive: vi.fn().mockResolvedValue([]),
    },
    systemWarnings: {
      getActive: vi.fn().mockResolvedValue([]),
    },
    ingestion: {
      cancelScan: vi.fn().mockResolvedValue({}),
    },
  },
}));

describe('SidebarNotificationCenter', () => {
  beforeEach(() => {
    useNotificationStore.setState({
      tasks: {},
      notifications: [],
      isExpanded: false,
    });
  });

  it('renders nothing when there are no active tasks or notifications', () => {
    const { container } = render(
      <MemoryRouter>
        <SidebarNotificationCenter />
      </MemoryRouter>
    );
    expect(container.firstChild).toBeNull();
  });

  it('renders active scan pipeline with progress bar and step counts', () => {
    useNotificationStore.setState({
      tasks: {
        'scan:acct_1': {
          id: 'scan:acct_1',
          domainKey: 'scan:acct_1',
          category: 'ingestion',
          title: 'Gmail Scan Pipeline',
          description: 'Found 15 txns, 2 stmts',
          status: 'running',
          current: 45,
          total: 100,
          progressPct: 45,
          etaSeconds: 30,
          startedAt: Date.now() - 5000,
          updatedAt: Date.now(),
        },
      },
    });

    render(
      <MemoryRouter>
        <SidebarNotificationCenter />
      </MemoryRouter>
    );

    expect(screen.getByText('1 Active Process')).toBeTruthy();
    expect(screen.getByText('Gmail Scan Pipeline')).toBeTruthy();
    expect(screen.getByText('45/100 (45%)')).toBeTruthy();
  });

  it('renders notification alerts feed items correctly', () => {
    useNotificationStore.setState({
      isExpanded: true,
      notifications: [
        {
          id: 'notif_1',
          category: 'normalization',
          severity: 'success',
          title: 'Normalization Complete',
          message: 'AI pass finished: 120 transactions normalized.',
          timestamp: Date.now() - 1000,
          read: false,
          dismissed: false,
        },
      ],
    });

    render(
      <MemoryRouter>
        <SidebarNotificationCenter />
      </MemoryRouter>
    );

    expect(screen.getByText('Normalization Complete')).toBeTruthy();
    expect(screen.getByText('AI pass finished: 120 transactions normalized.')).toBeTruthy();
  });

  it('allows expanding and collapsing the notifications drawer', () => {
    useNotificationStore.setState({
      tasks: {
        'task_1': {
          id: 'task_1',
          domainKey: 'task_1',
          category: 'statements',
          title: 'Statement Import Pipeline',
          description: 'Importing PDFs',
          status: 'running',
          current: 5,
          total: 10,
          progressPct: 50,
          startedAt: Date.now(),
          updatedAt: Date.now(),
        },
      },
      notifications: [
        {
          id: 'notif_1',
          category: 'database',
          severity: 'info',
          title: 'Backup Complete',
          message: 'Database snapshot taken.',
          timestamp: Date.now(),
          read: false,
          dismissed: false,
        },
      ],
    });

    render(
      <MemoryRouter>
        <SidebarNotificationCenter />
      </MemoryRouter>
    );

    const toggleBtn = screen.getByLabelText('Expand notifications');
    expect(toggleBtn).toBeTruthy();

    fireEvent.click(toggleBtn);
    expect(useNotificationStore.getState().isExpanded).toBe(true);
  });
});
