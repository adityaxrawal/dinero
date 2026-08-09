// No real email provider is provisioned yet (placeholder infra, Aditya's
// decision 2026-07-22). This interface lets every caller (deactivate,
// refund, ...) be written and tested now; swapping in a real provider
// (Resend/SendGrid/etc.) later is a body-of-this-function-only change.
import { maskEmail } from './license_key';

export interface EmailSender {
  send(params: { to: string; subject: string; body: string }): Promise<void>;
}

export const consoleEmailSender: EmailSender = {
  async send(params) {
    // TASK-OPS-007: this placeholder previously logged the raw recipient
    // address -- masked here for the same reason every other log line in
    // this backend never carries a raw identifier (request_logging.ts).
    console.log(`[email:placeholder] to=${maskEmail(params.to)} subject="${params.subject}"`);
  },
};
