/**
 * Recursive pretty-printer for the raw source payloads shown in the
 * transaction audit panel. Extracted from TransactionDetail.tsx so it can be
 * tested directly — it is pure, and the page around it is not.
 */
export function JsonViewer({ data }: { data: unknown }) {
  if (typeof data === 'string') return <span className="text-green-400 break-all">"{data}"</span>;
  if (typeof data === 'number') return <span className="text-orange-400">{data}</span>;
  if (typeof data === 'boolean') return <span className="text-purple-400">{data ? 'true' : 'false'}</span>;
  if (data === null || data === undefined) return <span className="text-muted-foreground">null</span>;
  if (Array.isArray(data)) {
    if (data.length === 0) return <span className="text-muted-foreground">[]</span>;
    return (
      <div className="pl-2 border-l border-border/40 ml-2">
        {data.map((item, index) => (
          <div key={index} className="flex">
            <JsonViewer data={item} />
            {index < data.length - 1 && <span className="text-muted-foreground">,</span>}
          </div>
        ))}
      </div>
    );
  }
  const entries = Object.entries(data as Record<string, unknown>);
  return (
    <div className="pl-2 border-l border-border/40 ml-2">
      {entries.map(([key, value], index) => (
        <div key={key} className="flex flex-wrap items-start">
          <span className="text-blue-400 font-medium mr-2">"{key}":</span>
          <JsonViewer data={value} />
          {index < entries.length - 1 && <span className="text-muted-foreground">,</span>}
        </div>
      ))}
    </div>
  );
}
