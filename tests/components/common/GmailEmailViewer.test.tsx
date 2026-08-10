// The Gmail message viewer. gmailParsing.test.ts already pins the parsers;
// this covers the component around them -- view-mode switching, the header
// vs. bare-switcher chrome, and the iframe auto-grow, which is the one piece
// no other spec reaches because jsdom never fires onLoad for an srcDoc frame.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GmailEmailViewer } from '@/components/common/GmailEmailViewer';

const HTML = '<p>Your card was charged INR 450.00</p>';
const TEXT = 'Your card was charged INR 450.00';

const iframe = () => screen.getByTitle('Gmail Email Content') as HTMLIFrameElement;

/** jsdom leaves contentDocument null for srcDoc frames, so stand one in. */
const stubBody = (el: HTMLIFrameElement, scrollHeight: number | undefined) =>
  Object.defineProperty(el, 'contentDocument', {
    value: scrollHeight === undefined ? {} : { body: { scrollHeight } },
    configurable: true,
  });

beforeEach(() => {
  vi.clearAllMocks();
});

describe('GmailEmailViewer view modes', () => {
  it('opens in Gmail view when the message has HTML', () => {
    render(<GmailEmailViewer html={HTML} text={TEXT} />);
    expect(iframe()).toBeInTheDocument();
  });

  it('opens in reader view when it does not', () => {
    render(<GmailEmailViewer text={TEXT} />);
    expect(screen.queryByTitle('Gmail Email Content')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Gmail View/ })).not.toBeInTheDocument();
  });

  it('honours an explicit initial mode over the default', () => {
    render(<GmailEmailViewer html={HTML} text={TEXT} initialViewMode="text" />);
    expect(screen.queryByTitle('Gmail Email Content')).not.toBeInTheDocument();
  });

  it('switches between reader, Gmail and plain text', () => {
    render(<GmailEmailViewer html={HTML} text={TEXT} />);

    fireEvent.click(screen.getByRole('button', { name: /Reader View/ }));
    expect(screen.queryByTitle('Gmail Email Content')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Gmail View/ }));
    expect(iframe()).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /^Text$/ }));
    expect(screen.queryByTitle('Gmail Email Content')).not.toBeInTheDocument();
    expect(screen.getByText(TEXT)).toBeInTheDocument();
  });

  it('says so when there is no body text at all', () => {
    render(<GmailEmailViewer initialViewMode="reader" />);
    expect(screen.getByText('No email body text available.')).toBeInTheDocument();
  });

  it('says so when there is nothing to show in text mode either', () => {
    render(<GmailEmailViewer initialViewMode="text" />);
    expect(screen.getByText('No email content available.')).toBeInTheDocument();
  });
});

describe('GmailEmailViewer iframe auto-grow', () => {
  it('starts at the default height', () => {
    render(<GmailEmailViewer html={HTML} />);
    expect(iframe().style.height).toBe('320px');
  });

  it('grows to the content height plus padding', () => {
    render(<GmailEmailViewer html={HTML} />);
    const el = iframe();
    stubBody(el, 500);
    fireEvent.load(el);
    expect(el.style.height).toBe('520px');
  });

  it('never shrinks below the 260px floor for short content', () => {
    render(<GmailEmailViewer html={HTML} />);
    const el = iframe();
    stubBody(el, 100);
    fireEvent.load(el);
    expect(el.style.height).toBe('260px');
  });

  it('ignores a suspiciously short body rather than collapsing the frame', () => {
    render(<GmailEmailViewer html={HTML} />);
    const el = iframe();
    stubBody(el, 50);
    fireEvent.load(el);
    expect(el.style.height).toBe('320px');
  });

  it('leaves the height alone when the body is unreadable', () => {
    render(<GmailEmailViewer html={HTML} />);
    const el = iframe();
    stubBody(el, undefined);
    fireEvent.load(el);
    expect(el.style.height).toBe('320px');
  });

  it('survives a cross-origin access throw', () => {
    render(<GmailEmailViewer html={HTML} />);
    const el = iframe();
    Object.defineProperty(el, 'contentDocument', {
      get() {
        throw new DOMException('cross-origin', 'SecurityError');
      },
      configurable: true,
    });
    expect(() => fireEvent.load(el)).not.toThrow();
    expect(el.style.height).toBe('320px');
  });
});

describe('GmailEmailViewer chrome', () => {
  it('shows the full header with sender and subject by default', () => {
    render(
      <GmailEmailViewer
        html={HTML}
        subject="Transaction alert"
        sender="HDFC Bank"
        senderEmail="alerts@hdfcbank.net"
        date="2026-07-26T10:30:00Z"
      />
    );
    expect(screen.getByText('Transaction alert')).toBeInTheDocument();
    expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
    expect(screen.getByText('<alerts@hdfcbank.net>')).toBeInTheDocument();
  });

  it('falls back to a bare switcher bar when the header is hidden', () => {
    render(<GmailEmailViewer html={HTML} subject="Transaction alert" showHeader={false} />);
    expect(screen.queryByText('Transaction alert')).not.toBeInTheDocument();
    expect(screen.getByText('View Mode')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Gmail View/ })).toBeInTheDocument();
  });

  it('renders no chrome at all when both are switched off', () => {
    render(<GmailEmailViewer html={HTML} showHeader={false} showViewModeSwitcher={false} />);
    expect(screen.queryByText('View Mode')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Reader View/ })).not.toBeInTheDocument();
  });

  it('shows an unparseable date verbatim instead of "Invalid Date"', () => {
    render(<GmailEmailViewer html={HTML} sender="HDFC" date="whenever" />);
    expect(screen.getByText('whenever')).toBeInTheDocument();
  });
});

describe('GmailEmailViewer quick-fill', () => {
  it('stays hidden unless the caller wants quick-fill', () => {
    render(<GmailEmailViewer html={HTML} text={TEXT} />);
    expect(screen.queryByText(/Quick-Fill/)).not.toBeInTheDocument();
  });

  it('offers the amounts it found and reports the picked field', () => {
    const onQuickFill = vi.fn();
    render(<GmailEmailViewer html={HTML} text={TEXT} onQuickFill={onQuickFill} />);

    const chip = screen.getByText('Amount: ₹450');
    fireEvent.click(chip);
    expect(onQuickFill).toHaveBeenCalledWith({ field: 'amount', value: '450.00' });
  });

  it('renders no bar when the message yields no candidates', () => {
    const onQuickFill = vi.fn();
    render(<GmailEmailViewer text="nothing useful here" onQuickFill={onQuickFill} />);
    expect(screen.queryByText(/Quick-Fill/)).not.toBeInTheDocument();
  });
});
