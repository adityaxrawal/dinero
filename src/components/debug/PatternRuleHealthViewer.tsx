import { useEffect, useState } from 'react';
import { API } from '../../lib/ipc';
import { Badge } from '../ui/badge';

import { DebugTableLayout } from './DebugTableLayout';

interface PatternRule {
  id: string;
  merchant_id: string;
  pattern_type: string;
  pattern_value: string;
  is_active: boolean;
  success_count: number;
  failure_count: number;
}

export function PatternRuleHealthViewer() {
  const [rules, setRules] = useState<PatternRule[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchRules = async () => {
    setLoading(true);
    try {
      const data = await API.debug.fetchPatternRuleHealth();
      setRules(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchRules();
  }, []);

  return (
    <DebugTableLayout
      title="Pattern Rule Health"
      onRefresh={fetchRules}
      loading={loading}
      data={rules}
      loadingMessage="Loading pattern rules..."
      emptyMessage="No pattern rules found."
      headers={
        <>
          <th className="p-2 text-sm font-medium text-muted-foreground">ID</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Merchant</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Type</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Value</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Status</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Success</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Failure</th>
        </>
      }
      renderRow={(rule) => (
        <tr key={rule.id} className="border-b border-[var(--border-color)] last:border-0">
          <td className="p-2 text-sm font-mono">{rule.id.substring(0, 8)}</td>
          <td className="p-2 text-sm">{rule.merchant_id}</td>
          <td className="p-2 text-sm">
            <Badge variant="outline">{rule.pattern_type}</Badge>
          </td>
          <td className="p-2 text-sm">{rule.pattern_value}</td>
          <td className="p-2 text-sm">
            {rule.is_active ? (
              <Badge variant="default" className="bg-green-600">
                Active
              </Badge>
            ) : (
              <Badge variant="destructive">Inactive</Badge>
            )}
          </td>
          <td className="p-2 text-sm text-green-700 font-bold">{rule.success_count}</td>
          <td className="p-2 text-sm text-red-500 font-bold">{rule.failure_count}</td>
        </tr>
      )}
    />
  );
}
