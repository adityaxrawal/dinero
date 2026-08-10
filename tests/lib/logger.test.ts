// The frontend logger mirrors everything to the Rust side over IPC. Its
// level routing, payload serialization, and -- most importantly -- its
// refusal to throw when that IPC call fails are pinned here: a logger that
// throws turns one failure into a recursive failure loop.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { logger } from '@/lib/logger';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const lastPayload = () => asMock(invoke).mock.calls[0][1] as Record<string, unknown>;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
  asMock(invoke).mockResolvedValue(undefined);
  for (const m of ['debug', 'log', 'warn', 'error'] as const) {
    vi.spyOn(console, m).mockImplementation(() => {});
  }
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('logger level routing', () => {
  it.each([
    ['debug', 'debug'],
    ['info', 'log'],
    ['warn', 'warn'],
    ['error', 'error'],
  ])('sends %s to the matching console method and over IPC', async (level, consoleMethod) => {
    (logger as unknown as Record<string, (m: string) => void>)[level]('a message');
    await flush();

    expect(console[consoleMethod as 'debug']).toHaveBeenCalledWith('[FRONTEND]', 'a message', '');
    expect(invoke).toHaveBeenCalledWith('log_frontend_event', expect.anything());
    expect(lastPayload()).toMatchObject({ level, message: 'a message', target: 'frontend' });
  });

  it('uppercases a custom target in the console prefix but keeps it raw in the payload', async () => {
    logger.info('scan started', undefined, 'network');
    await flush();

    expect(console.log).toHaveBeenCalledWith('[NETWORK]', 'scan started', '');
    expect(lastPayload().target).toBe('network');
  });
});

describe('logger payload serialization', () => {
  it('passes a string through untouched', async () => {
    logger.info('m', 'already-a-string');
    await flush();
    expect(lastPayload().data).toBe('already-a-string');
  });

  it('serializes structured data as JSON', async () => {
    logger.info('m', { a: 1, b: [2] });
    await flush();
    expect(lastPayload().data).toBe('{"a":1,"b":[2]}');
  });

  it('sends null rather than "undefined" when there is no data', async () => {
    logger.info('m');
    await flush();
    expect(lastPayload().data).toBeNull();
  });
});

describe('logger failure isolation', () => {
  it('swallows an IPC rejection instead of letting it escape', async () => {
    asMock(invoke).mockRejectedValue(new Error('backend down'));
    expect(() => logger.error('still fine')).not.toThrow();
    await flush();
    // The console half must still have happened -- losing the IPC sink is
    // not a reason to lose the local one too.
    expect(console.error).toHaveBeenCalled();
  });
});

describe('logger.apiCall', () => {
  it('reports a successful command at info level', async () => {
    logger.apiCall('list_transactions', { page: 1 }, 12.345, true);
    await flush();

    const p = lastPayload();
    expect(p.level).toBe('info');
    expect(p.target).toBe('api_calls');
    expect(p.message).toBe('IPC Command: list_transactions [SUCCESS] (12.3ms)');
  });

  it('reports a failed command at error level and keeps the error detail', async () => {
    logger.apiCall('list_transactions', {}, 4, false, 'boom');
    await flush();

    const p = lastPayload();
    expect(p.level).toBe('error');
    expect(p.message).toContain('[FAILED]');
    expect(JSON.parse(p.data as string).error).toBe('boom');
  });
});

describe('logger.network', () => {
  it('treats a 2xx as info', async () => {
    logger.network('https://api.test/x', 'GET', 200, 30);
    await flush();

    const p = lastPayload();
    expect(p.level).toBe('info');
    expect(p.message).toBe('Outbound HTTP GET https://api.test/x - Status: 200 (30.0ms)');
  });

  it('treats a 4xx/5xx as an error', async () => {
    logger.network('https://api.test/x', 'POST', 503, 12);
    await flush();
    expect(lastPayload().level).toBe('error');
  });

  it('treats a missing status as an error and labels it ERR', async () => {
    logger.network('https://api.test/x', 'GET', null, 5);
    await flush();

    const p = lastPayload();
    expect(p.level).toBe('error');
    expect(p.message).toContain('Status: ERR');
  });
});

describe('logger.userAction', () => {
  it('names the target element when there is one', async () => {
    logger.userAction('clicked', 'save-button');
    await flush();
    expect(lastPayload().message).toBe('User Action: clicked on [save-button]');
  });

  it('omits the bracket entirely when there is not', async () => {
    logger.userAction('clicked');
    await flush();
    expect(lastPayload().message).toBe('User Action: clicked');
  });
});

describe('logger outside development', () => {
  it('stays off the console but still ships to the backend', async () => {
    vi.stubEnv('DEV', false);
    vi.resetModules();
    const { logger: prodLogger } = await import('@/lib/logger');

    prodLogger.error('quiet failure');
    await flush();

    expect(console.error).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith(
      'log_frontend_event',
      expect.objectContaining({ level: 'error', message: 'quiet failure' })
    );
    vi.unstubAllEnvs();
  });
});
