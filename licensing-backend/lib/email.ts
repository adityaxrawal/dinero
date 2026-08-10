/**
 * Outbound email abstraction for the licensing service.
 *
 * Only a console placeholder exists today; the interface is the seam a real
 * provider would slot into. Note the address is masked even in a log line --
 * these logs are operational and should not accumulate readable user emails.
 */
import { maskEmail } from './license_key';

export interface EmailSender {
  send(params: { to: string; subject: string; body: string }): Promise<void>;
}

export const consoleEmailSender: EmailSender = {
  async send(params) {
    console.log(`[email:placeholder] to=${maskEmail(params.to)} subject="${params.subject}"`);
  },
};
