import type { UnprocessedStatementEntry } from '@/lib/ipc';

export function formatUnprocessedStatementName(item: UnprocessedStatementEntry): string {
  const banks = [
    'HDFC', 'ICICI', 'SBI', 'AXIS', 'AMEX', 'AMERICAN EXPRESS',
    'KOTAK', 'INDUSIND', 'YES BANK', 'CITI', 'IDFC', 'RBL', 'HSBC', 'STANDARD CHARTERED'
  ];

  const searchStr = `${item.sender || ''} ${item.subject || ''} ${item.snippet || ''}`.toUpperCase();

  let bankName = banks.find(b => searchStr.includes(b)) || 'UNKNOWN';
  if (bankName === 'AMERICAN EXPRESS') bankName = 'AMEX';
  if (bankName === 'STANDARD CHARTERED') bankName = 'SCB';
  if (bankName === 'YES BANK') bankName = 'YES';
  
  // Clean bankName to remove spaces if any (though currently only YES is the one, maybe others like YES BANK)
  bankName = bankName.replace(/\s+/g, '');

  let last4Match = searchStr.match(/(?:ENDING\s+(?:IN\s+)?|XX+|X\s*X\s*X\s*X|\*\*+|ACCOUNT\s+|CARD\s+|A\/C\s+NO\.?\s*)([0-9]{4})\b/);
  let last4 = last4Match ? last4Match[1] : null;

  if (!last4) {
    const fourDigits = ((item.subject || '').match(/\b(\d{4})\b/g) || []);
    const nonYear = fourDigits.find(d => {
       const n = parseInt(d, 10);
       return n < 2000 || n > 2050; 
    });
    if (nonYear) last4 = nonYear;
  }
  
  if (!last4) {
    const fourDigits = ((item.snippet || '').match(/\b(\d{4})\b/g) || []);
    const nonYear = fourDigits.find(d => {
       const n = parseInt(d, 10);
       return n < 2000 || n > 2050; 
    });
    if (nonYear) last4 = nonYear;
  }
  
  if (!last4) last4 = 'XXXX';

  let month = 'MMM';
  let year = 'YYYY';

  if (item.date) {
    const d = new Date(item.date);
    if (!isNaN(d.getTime())) {
      const months = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];
      month = months[d.getMonth()];
      year = d.getFullYear().toString();
    }
  }

  if (month === 'MMM' || year === 'YYYY') {
    const monthMatch = searchStr.match(/\b(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)[A-Z]*\b/);
    if (monthMatch) month = monthMatch[1];

    const yearMatch = searchStr.match(/\b(20[1-3][0-9])\b/);
    if (yearMatch) year = yearMatch[1];
  }
  
  const result = `${bankName}BANKXXXX${last4}${month}${year}`;
  
  if (result === 'UNKNOWNBANKXXXXXXXXMMMYYYY') {
     return item.filename || 'Unknown file';
  }
  
  return result;
}
