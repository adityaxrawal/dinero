/**
 * Statement management: upload, processing history, and the retry queue.
 */
import { useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { AlertTriangle, FileSearch, Upload } from 'lucide-react';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { queryKeys } from '@/lib/queryKeys';
import { PageSidebar } from '@/components/layout/PageSidebar';
import SectionHeading from '@/components/settings/SectionHeading';
import StatementUploadDropzone from '@/components/statements/StatementUploadDropzone';
import UnprocessedItemsQueue from '@/components/statements/UnprocessedItemsQueue';
import PasswordPromptModal from '@/components/statements/PasswordPromptModal';
import StatementReviewModal from '@/components/statements/StatementReviewModal';
import { useInstrumentGate } from './statements/useInstrumentGate';
import InstrumentGateDialog from './statements/InstrumentGateDialog';
import ProcessingHistorySection from './statements/ProcessingHistorySection';

type StatementsSection = 'upload' | 'queue' | 'history';

const SECTIONS = [
  { id: 'upload', label: 'Upload Statements', icon: Upload },
  { id: 'queue', label: 'Action Needed', icon: AlertTriangle },
  { id: 'history', label: 'Processing History', icon: FileSearch },
] as const;

/** Statement upload, history and the retry queue. */
export default function Statements() {
  const [searchParams, setSearchParams] = useSearchParams();
  const sectionParam = searchParams.get('section');
  const currentSection: StatementsSection =
    sectionParam === 'queue' || sectionParam === 'history' ? sectionParam : 'upload';
  /** Switches the visible statements section. */
  const setSection = (section: StatementsSection) => setSearchParams({ section });

  const { openPasswordModal } = useGlobalState();
  const queryClient = useQueryClient();
  const refresh = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
  }, [queryClient]);

  const gate = useInstrumentGate(refresh);

  return (
    <div className="flex h-full w-full overflow-hidden">
      <PageSidebar
        title="Statements"
        sections={SECTIONS}
        currentSection={currentSection}
        onSelectSection={setSection}
      />

      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12 text-[#064E3B]">
        <div className="max-w-3xl mx-auto space-y-8">
          {currentSection === 'upload' && (
            <section aria-label="Upload Statements" className="animate-in fade-in duration-300">
              <SectionHeading
                icon={Upload}
                title="Upload Statements"
                description="Drop PDF statements here to parse and extract transactions."
              />
              <StatementUploadDropzone onUploaded={refresh} />
            </section>
          )}

          {currentSection === 'queue' && (
            <section aria-label="Action Needed" className="animate-in fade-in duration-300">
              <SectionHeading
                icon={AlertTriangle}
                iconClassName="text-amber-600"
                title="Action Needed"
                description="Statements requiring a password or further attention."
              />
              <UnprocessedItemsQueue
                onEnterPassword={(statementId) => openPasswordModal(statementId)}
              />
            </section>
          )}

          {currentSection === 'history' && <ProcessingHistorySection refresh={refresh} />}
        </div>
      </div>

      <PasswordPromptModal onUnlocked={refresh} />

      <StatementReviewModal />

      <InstrumentGateDialog gate={gate} />
    </div>
  );
}
