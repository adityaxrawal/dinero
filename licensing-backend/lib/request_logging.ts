// Doc 30 TASK-OPS-007: "Standardized structured logs across desktop and
// Licensing Backend code paths (request ID, endpoint, status, latency,
// redacted identifiers only)." Deliberately logs only these four fields --
// never the request body, response body, or any header -- so there is
// nothing to redact after the fact: an account email, license JWT, or
// Razorpay payment id can never end up in a log line in the first place,
// which is a stronger guarantee than logging-then-redacting.
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { randomUUID } from 'crypto';

export interface RequestLogLine {
  request_id: string;
  endpoint: string;
  status: number;
  latency_ms: number;
}

/// Wraps a Vercel handler so every call logs exactly one structured JSON
/// line (via `console.log`, which Vercel's platform captures and retains --
/// this backend has no separate log file of its own to rotate). The
/// `X-Request-Id` response header lets a user's own support ticket be
/// correlated back to a specific log line without exposing anything about
/// the account itself.
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
