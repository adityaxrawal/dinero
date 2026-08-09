import type { InstrumentRecord } from '@/lib/ipc';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import { usePdfPasswordsList } from '@/hooks/queries/usePdfPasswordsList';

/** Everything filed under one instrument: its transactions, statements and
 *  saved statement passwords. */
export function useInstrumentRelated(
  instrumentId: string | undefined,
  inst: InstrumentRecord | undefined
) {
  const {
    data: txData,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading: isTxLoading,
  } = useTransactionsInfiniteList(instrumentId ? { instrument_id: instrumentId } : {});

  const { data: statements = [] } = useStatementsList();
  const { data: pdfPasswords = [] } = usePdfPasswordsList();

  const recentTransactions = txData?.pages.flatMap((page) => page.records) ?? [];
  const instrumentId_ = inst?.id;

  return {
    recentTransactions,
    totalTxCount: txData?.pages[0]?.total ?? recentTransactions.length,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isTxLoading,
    instrumentStatements: statements.filter((s) => s.instrument_id === instrumentId_),
    instrumentPasswords: pdfPasswords.filter((p) => p.instrument_id === instrumentId_),
  };
}
