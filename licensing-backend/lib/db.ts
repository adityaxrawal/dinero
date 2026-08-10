/**
 * Prisma client singleton and account lookup.
 *
 * The global cache exists because of serverless development reloads: each hot
 * reload would otherwise construct a new PrismaClient and open another
 * connection pool until the database refuses new connections. Production skips
 * the global, since each cold start legitimately gets its own client.
 */
import type { PrismaClient, Account } from '@prisma/client';
import { PrismaClient as PrismaClientCtor } from '@prisma/client';

declare global {
  var __dineroPrisma: PrismaClient | undefined;
}

export const prisma: PrismaClient = global.__dineroPrisma ?? new PrismaClientCtor();

if (process.env.NODE_ENV !== 'production') {
  global.__dineroPrisma = prisma;
}

/**
 * Returns the account for an email, creating it if absent.
 */
export async function findOrCreateAccount(
  db: { account: Pick<PrismaClient['account'], 'findUnique' | 'create'> },
  email: string
): Promise<Account> {
  const account = await db.account.findUnique({ where: { email } });
  if (account) return account;
  return db.account.create({ data: { email } });
}
