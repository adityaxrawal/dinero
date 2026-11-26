/**
 * Date-range picker bounding a historical scan.
 */
import { DateRangePicker } from '@/components/ui/date-picker';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** Date-range picker bounding a historical scan. */
export default function ScanRangePicker({ disabled }: { disabled: boolean }) {
  const {
    scanStartDate,
    setScanStartDate,
    scanEndDate,
    setScanEndDate,
    recheckRejected,
    setRecheckRejected,
  } = useGlobalState();

  return (
    <div className="p-4 rounded-xl bg-champagne/40 border border-emerald-ink/10">
      <DateRangePicker
        startDate={scanStartDate}
        endDate={scanEndDate}
        onChange={({ startDate, endDate }) => {
          setScanStartDate(startDate);
          setScanEndDate(endDate);
        }}
        disabled={disabled}
      />
      <label className="mt-3 flex items-center gap-2 text-[12px] text-emerald-ink/80">
        <input
          type="checkbox"
          checked={recheckRejected}
          onChange={(e) => setRecheckRejected(e.target.checked)}
          disabled={disabled}
        />
        Re-check emails that earlier scans rejected
      </label>
    </div>
  );
}
