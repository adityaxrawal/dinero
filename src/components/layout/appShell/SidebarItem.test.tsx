// The rail item is rendered for every route, and its badge is the only place
// the unresolved-cluster count surfaces outside the Reconciliation page.
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import SidebarItem, { type NavItem } from './SidebarItem';

const item = (over: Partial<NavItem> = {}): NavItem => ({
  path: '/reconciliation',
  label: 'Review Inbox',
  icon: <span data-testid="icon" />,
  badge: 0,
  ...over,
});

const renderItem = (props: NavItem, isActive = false) =>
  render(
    <MemoryRouter>
      <SidebarItem item={props} isActive={isActive} />
    </MemoryRouter>
  );

describe('SidebarItem', () => {
  it('links to its route with its label and icon', () => {
    renderItem(item());
    expect(screen.getByRole('link')).toHaveAttribute('href', '/reconciliation');
    expect(screen.getByText('Review Inbox')).toBeInTheDocument();
    expect(screen.getByTestId('icon')).toBeInTheDocument();
  });

  it('hides a zero or absent badge', () => {
    const { unmount } = renderItem(item({ badge: 0 }));
    expect(screen.getByRole('link')).toHaveAttribute('aria-label', 'Review Inbox');
    unmount();

    const noBadge = item();
    delete noBadge.badge;
    renderItem(noBadge);
    expect(screen.getByRole('link')).toHaveAttribute('aria-label', 'Review Inbox');
  });

  it('announces a pending count in the link label', () => {
    renderItem(item({ badge: 3 }));
    expect(screen.getByRole('link')).toHaveAttribute('aria-label', 'Review Inbox — 3 pending');
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('caps the badge at 9+ so the rail cannot be pushed wide', () => {
    renderItem(item({ badge: 42 }));
    expect(screen.getByText('9+')).toBeInTheDocument();
  });

  it('marks the active route as the current page', () => {
    renderItem(item(), true);
    expect(screen.getByRole('link').querySelector('[aria-current="page"]')).toBeTruthy();
  });

  it('scales the badge while a pulse is in flight', () => {
    renderItem(item({ badge: 2, pulse: true }));
    const badge = screen.getByTestId('reconciliation-badge');
    expect(badge).toHaveAttribute('data-pulsing', 'true');
    expect(badge.className).toContain('scale-125');
  });
});
