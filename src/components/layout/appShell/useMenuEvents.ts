/**
 * Acts on the native macOS menu and menu-bar extra (tray). The Rust side only
 * emits `menu_*` events; nothing listened, so these menu items did nothing
 * (audit F20-1). There is no Toggle Sidebar item: the sidebar hosts the
 * licence-lock and system banners, so it has no collapsed mode.
 */
import { useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { useIpcListen } from '@/hooks/useIpcListen';
import { useToast } from '@/hooks/use-toast';
import { queryKeys } from '@/lib/queryKeys';
import { useStatementUpload } from '@/components/statements/upload/useStatementUpload';

export function useMenuEvents(): void {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const { pickAndUpload } = useStatementUpload(() =>
    queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() })
  );

  useIpcListen<{ route: string }>('menu_navigate', ({ route }) => navigate(route));
  useIpcListen('menu_upload_statement_requested', () => {
    navigate('/statements');
    pickAndUpload();
  });
  // The check itself runs in Rust; an available update arrives as
  // `update_available`. This only confirms the click did something.
  useIpcListen('menu_check_for_updates', () =>
    toast({ title: 'Checking for updates…', description: "You'll be told if one is available." })
  );
}
