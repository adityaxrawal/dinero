import { useEffect, useState } from 'react';
import { API } from '../../lib/ipc';
import { Badge } from '../ui/badge';

import { DebugTableLayout } from './DebugTableLayout';

export function UnprocessedStatementViewer() {
  const [statements, setStatements] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchStatements = async () => {
    setLoading(true);
    try {
      const data = await API.debug.fetchUnprocessedStatements();
      setStatements(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStatements();
  }, []);



  return (
    <DebugTableLayout
      title="Unprocessed Statements"
      onRefresh={fetchStatements}
      loading={loading}
      data={statements}
      loadingMessage="Loading statements..."
      emptyMessage="No unprocessed statements found."
      headers={
        <>
          <th className="p-2 text-sm font-medium text-muted-foreground">ID</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Created At</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Original Filename</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">File Hash</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Status</th>
        </>
      }
      renderRow={stmt => (
        <tr key={stmt.id} className="border-b border-[var(--border-color)] last:border-0">
          <td className="p-2 text-sm font-mono">{stmt.id.substring(0, 8)}</td>
          <td className="p-2 text-sm">{new Date(stmt.created_at).toLocaleString()}</td>
          <td className="p-2 text-sm">{stmt.original_filename}</td>
          <td className="p-2 text-sm font-mono text-muted-foreground">{stmt.file_hash.substring(0, 12)}...</td>
          <td className="p-2 text-sm">
            {stmt.needs_password ? (
              <Badge variant="destructive">Needs Password</Badge>
            ) : (
              <Badge variant="outline">Pending</Badge>
            )}
          </td>
        </tr>
      )}
    />
  );
}
