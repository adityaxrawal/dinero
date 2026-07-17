import React from 'react';

interface DebugTableLayoutProps<T> {
  title: React.ReactNode;
  onRefresh: () => void;
  loading: boolean;
  data: T[];
  loadingMessage: string;
  emptyMessage: string;
  headers: React.ReactNode;
  renderRow: (item: T) => React.ReactNode;
  headerActions?: React.ReactNode;
}

export function DebugTableLayout<T>({
  title,
  onRefresh,
  loading,
  data,
  loadingMessage,
  emptyMessage,
  headers,
  renderRow,
  headerActions
}: DebugTableLayoutProps<T>) {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex justify-between items-center">
        <h2 className="heading-md">{title}</h2>
        <div className="flex items-center gap-2">
          {headerActions}
          <button className="btn btn-secondary text-sm" onClick={onRefresh}>Refresh</button>
        </div>
      </div>
      
      {loading ? (
        <div>{loadingMessage}</div>
      ) : data.length === 0 ? (
        <div className="p-8 text-center text-muted-foreground border border-dashed border-[var(--border-color)] rounded-[var(--radius-md)]">
          {emptyMessage}
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="border-b border-[var(--border-color)]">
                {headers}
              </tr>
            </thead>
            <tbody>
              {data.map(renderRow)}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
