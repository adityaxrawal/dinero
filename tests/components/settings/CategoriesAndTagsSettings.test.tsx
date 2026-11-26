// Category and tag management had backend commands but no UI (audit F21-1).
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import CategoriesAndTagsSettings from '@/components/settings/CategoriesAndTagsSettings';

const create = vi.fn();
const del = vi.fn();
const deleteTag = vi.fn();
const toast = vi.fn();

vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries: vi.fn() }) }));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/confirmDialog', () => ({ confirmAction: () => Promise.resolve(true) }));
vi.mock('@/hooks/queries/useCategoriesList', () => ({
  useCategoriesList: () => ({
    data: [
      { id: 'sys', name: 'Food', source_type: 'system', is_deleted: false },
      { id: 'mine', name: 'Pet care', source_type: 'user', is_deleted: false },
    ],
  }),
}));
vi.mock('@/hooks/queries/useTagsList', () => ({
  useTagsList: () => ({ data: [{ id: 't1', name: 'trip' }] }),
}));
vi.mock('@/lib/ipc', () => ({
  API: {
    categories: { create: (...a: unknown[]) => create(...a), rename: vi.fn(), delete: (...a: unknown[]) => del(...a) },
    tags: { delete: (...a: unknown[]) => deleteTag(...a) },
  },
}));

beforeEach(() => {
  vi.clearAllMocks();
  create.mockResolvedValue({});
  del.mockResolvedValue({});
  deleteTag.mockResolvedValue('deleted');
});

describe('CategoriesAndTagsSettings', () => {
  it('lists only custom categories and adds a new one', async () => {
    render(<CategoriesAndTagsSettings />);
    expect(screen.getByText('Pet care')).toBeInTheDocument();
    expect(screen.queryByText('Food')).toBeNull();
    fireEvent.change(screen.getByLabelText('New category name'), { target: { value: 'Gifts' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add category' }));
    await waitFor(() => expect(create).toHaveBeenCalledWith('Gifts'));
  });

  it('deletes a category and a tag after confirmation', async () => {
    render(<CategoriesAndTagsSettings />);
    fireEvent.click(screen.getByLabelText('Delete category Pet care'));
    await waitFor(() => expect(del).toHaveBeenCalledWith('mine'));
    fireEvent.click(screen.getByLabelText('Delete tag trip'));
    await waitFor(() => expect(deleteTag).toHaveBeenCalledWith('t1'));
  });
});
