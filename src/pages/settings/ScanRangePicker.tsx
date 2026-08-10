/**
 * Date-range picker bounding a historical scan.
 */
import { DateRangePicker } from '@/components/ui/date-picker';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** Date-range picker bounding a historical scan. */
export default function ScanRangePicker({ disabled }: { disabled: boolean }) {
  const { scanStartDate, setScanStartDate, scanEndDate, setScanEndDate } = useGlobalState();

  return (
    <div className="p-4 rounded-xl bg-[#F8E7C9]/40 border border-[#064E3B]/10">
      <DateRangePicker
        startDate={scanStartDate}
        endDate={scanEndDate}
        onChange={({ startDate, endDate }) => {
          setScanStartDate(startDate);
          setScanEndDate(endDate);
        }}
        disabled={disabled}
      />
    </div>
  );
}
