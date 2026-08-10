/**
 * Wraps an endpoint so every request emits one structured log line.
 *
 * The correlation id is generated per request and returned in the X-Request-Id
 * header before the handler runs, so a user reporting a failure can quote an id
 * that appears in the logs.
 *
 * Logging happens in a `finally`, which is what makes it unconditional: a
 * handler that throws is still recorded, with whatever status the framework
 * settled on. The line is JSON so it can be queried by field rather than grepped.
 */
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { randomUUID } from 'crypto';

interface RequestLogLine {
  request_id: string;
  endpoint: string;
  status: number;
  latency_ms: number;
}

/**
 * Wraps a handler so every request emits one correlated, timed log line.
 */
export function withRequestLogging(
  endpoint: string,
  handler: (req: VercelRequest, res: VercelResponse) => Promise<void> | void
) {
  return async (req: VercelRequest, res: VercelResponse): Promise<void> => {
    const requestId = randomUUID();
    const start = Date.now();
    res.setHeader('X-Request-Id', requestId);
    try {
      await handler(req, res);
    } finally {
      const line: RequestLogLine = {
        request_id: requestId,
        endpoint,
        status: res.statusCode,
        latency_ms: Date.now() - start,
      };
      console.log(JSON.stringify(line));
    }
  };
}
