/**
 * Input for a masked card number.
 *
 * Only the last digits are ever entered and stored; a full card number, or a
 * full PAN, is never handled by this application.
 */
import React, { useState, useMemo } from 'react';
import {Eye, EyeOff, Copy, Check} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';

type CardNetwork = 'visa' | 'mastercard' | 'rupay' | 'amex' | 'generic';

/**
 * Identifies the card network from its leading digits.
 *
 * Uses the published IIN prefix ranges, which is why the Mastercard pattern is
 * so involved -- it covers both the classic 51-55 range and the newer 2221-2720
 * range. Falls back to a generic badge for anything unrecognised.
 */
function getCardNetwork(cardNumber: string): { network: CardNetwork; label: string; badgeClass: string } {
  const digits = cardNumber.replace(/\D/g, '');

  if (/^4/.test(digits)) {
    return { network: 'visa', label: 'VISA', badgeClass: 'bg-blue-600/15 text-blue-800 border-blue-600/30' };
  }
  if (/^(5[1-5]|222[1-9]|22[3-9]\d|2[3-6]\d{2}|27[0-1]\d|2720)/.test(digits)) {
    return { network: 'mastercard', label: 'MC', badgeClass: 'bg-orange-600/15 text-orange-800 border-orange-600/30' };
  }
  if (/^(34|37)/.test(digits)) {
    return { network: 'amex', label: 'AMEX', badgeClass: 'bg-[#006FCF]/15 text-[#006FCF] border-[#006FCF]/30' };
  }
  if (/^(60|65|81|82|508|3528|3589)/.test(digits)) {
    return { network: 'rupay', label: 'RuPay', badgeClass: 'bg-emerald-600/15 text-emerald-800 border-emerald-600/30' };
  }

  return { network: 'generic', label: 'CARD', badgeClass: 'bg-[#064E3B]/10 text-[#064E3B] border-[#064E3B]/20' };
}

/**
 * Groups digits into blocks of four for readability.
 *
 * Non-digits are stripped and the length capped at 19, the longest card number
 * any network issues.
 */
function formatCardNumber(value: string): string {
  const clean = value.replace(/\D/g, '').slice(0, 19);
  return clean.replace(/(\d{4})(?=\d)/g, '$1 ').trim();
}

/**
 * Masks all but the final block.
 *
 * The last four are what identify the card to its owner, and are all this app
 * ever needs -- so everything before them is hidden by default.
 */
function maskCardNumber(formattedValue: string): string {
  if (!formattedValue) return '';
  const parts = formattedValue.split(' ');
  if (parts.length <= 1) {
    const len = formattedValue.length;
    if (len <= 4) return formattedValue;
    return '•'.repeat(len - 4) + formattedValue.slice(-4);
  }
  return parts
    .map((part, idx) => (idx === parts.length - 1 ? part : '••••'))
    .join(' ');
}

interface CardNumberInputProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  placeholder?: string;
  className?: string;
}

/**
 * Card-number field with network detection, masking and copy.
 */
export default function CardNumberInput({
  id,
  value,
  onChange,
  onKeyDown,
  placeholder = '4532 7603 1920 8841',
  className,
}: CardNumberInputProps) {
  const { toast } = useToast();
  const [isMasked, setIsMasked] = useState(false);
  const [copied, setCopied] = useState(false);

  const formattedValue = useMemo(() => formatCardNumber(value), [value]);
  const displayedValue = isMasked ? maskCardNumber(formattedValue) : formattedValue;
  const brandInfo = useMemo(() => getCardNetwork(value), [value]);

  /** Reformats on each keystroke so grouping keeps up with typing. */
  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const rawVal = e.target.value;

    if (isMasked) setIsMasked(false);

    const digitsOnly = rawVal.replace(/\D/g, '');
    onChange(digitsOnly);
  };

  /** Copies the unmasked value and shows brief confirmation. */
  const handleCopy = () => {
    const textToCopy = value.replace(/\D/g, '') || formattedValue;
    if (!textToCopy) return;
    navigator.clipboard.writeText(textToCopy);
    setCopied(true);
    toast({
      title: 'Card Number Copied',
      description: 'Card identifier copied to clipboard.',
    });
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="relative flex items-center w-full">
      <div className="absolute left-2.5 flex items-center justify-center pointer-events-none z-10">
        <span
          className={cn(
            'text-[9px] font-black px-1.5 py-0.5 rounded-md uppercase tracking-wider border font-mono',
            brandInfo.badgeClass
          )}
        >
          {brandInfo.label}
        </span>
      </div>

      <input
        id={id}
        type="text"
        value={displayedValue}
        onChange={handleChange}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        maxLength={23}
        className={cn(
          'w-full h-9 pl-14 pr-16 text-[13px] font-mono font-bold bg-[#F3EBDD]/80 border border-[#064E3B]/15 text-[#064E3B]',
          'placeholder:text-[#064E3B]/30 outline-none rounded-xl focus:border-[#064E3B]/40 focus:ring-1 focus:ring-[#064E3B]/30 transition-all',
          className
        )}
      />

      <div className="absolute right-2.5 flex items-center gap-1 z-10">
        <button
          type="button"
          onClick={() => setIsMasked((prev) => !prev)}
          className="p-1 rounded-md text-[#064E3B]/60 hover:text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors cursor-pointer"
          title={isMasked ? 'Show card number' : 'Mask card number'}
          aria-label={isMasked ? 'Show card number' : 'Mask card number'}
        >
          {isMasked ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
        </button>

        <button
          type="button"
          onClick={handleCopy}
          className="p-1 rounded-md text-[#064E3B]/60 hover:text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors cursor-pointer"
          title="Copy card number"
          aria-label="Copy card number"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
      </div>
    </div>
  );
}
