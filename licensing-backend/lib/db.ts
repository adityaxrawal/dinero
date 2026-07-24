// Doc 30 TASK-LIC-001: single Prisma client instance, reused across
// serverless invocations within the same warm lambda (Vercel's documented
// pattern -- a fresh client per invocation exhausts Neon's connection limit
// under load).
import type { PrismaClient, Account } from '@prisma/client';
import { PrismaClient as PrismaClientCtor } from '@prisma/client';

declare global {
  // eslint-disable-next-line no-var
  var __dineroPrisma: PrismaClient | undefined;
}

export const prisma: PrismaClient = global.__dineroPrisma ?? new PrismaClientCtor();

if (process.env.NODE_ENV !== 'production') {
  global.__dineroPrisma = prisma;
}

// Doc 30 TASK-BILL-002: both license activation and order creation
// find-or-create the account the same way -- there is no separate signup
// step, the first Razorpay payment or order request IS the signup.
export async function findOrCreateAccount(
  db: { account: Pick<PrismaClient['account'], 'findUnique' | 'create'> },
  email: string
): Promise<Account> {
  const account = await db.account.findUnique({ where: { email } });
  if (account) return account;
  return db.account.create({ data: { email } });
}
