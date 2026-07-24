import { useEffect, useState } from 'react';
import { API } from '../../lib/ipc';

import { DebugTableLayout } from './DebugTableLayout';

interface ParseError {
  id: string;
  instrument_id: string | null;
  created_at: string;
  raw_payload_json: string | null;
}

export function ParseErrorViewer() {
  const [errors, setErrors] = useState<ParseError[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchErrors = async () => {
    setLoading(true);
    try {
      const data = await API.debug.fetchParseErrors();
      setErrors(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchErrors();
  }, []);

  return (
    <DebugTableLayout
      title="Parse Errors"
      onRefresh={fetchErrors}
      loading={loading}
      data={errors}
      loadingMessage="Loading parse errors..."
      emptyMessage="No parse errors found."
      headers={
        <>
          <th className="p-2 text-sm font-medium text-muted-foreground">ID</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Instrument ID</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Created At</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Raw Payload</th>
        </>
      }
      renderRow={(err) => (
        <tr key={err.id} className="border-b border-[var(--border-color)] last:border-0">
          <td className="p-2 text-sm font-mono">{err.id.substring(0, 8)}</td>
          <td className="p-2 text-sm">{err.instrument_id}</td>
          <td className="p-2 text-sm">{new Date(err.created_at).toLocaleString()}</td>
          <td className="p-2 text-sm">
            <pre className="max-w-md overflow-x-auto p-2 bg-black/20 rounded text-xs">
              {err.raw_payload_json || 'N/A'}
            </pre>
          </td>
        </tr>
      )}
    />
  );
}
