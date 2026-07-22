// Doc 30 TASK-LIC-001: single Prisma client instance, reused across
// serverless invocations within the same warm lambda (Vercel's documented
// pattern -- a fresh client per invocation exhausts Neon's connection limit
// under load).
import { PrismaClient } from '@prisma/client';

declare global {
  // eslint-disable-next-line no-var
  var __dineroPrisma: PrismaClient | undefined;
}

export const prisma: PrismaClient = global.__dineroPrisma ?? new PrismaClient();

if (process.env.NODE_ENV !== 'production') {
  global.__dineroPrisma = prisma;
}
