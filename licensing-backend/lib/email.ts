// No real email provider is provisioned yet (placeholder infra, Aditya's
// decision 2026-07-22). This interface lets every caller (deactivate,
// refund, ...) be written and tested now; swapping in a real provider
// (Resend/SendGrid/etc.) later is a body-of-this-function-only change.
export interface EmailSender {
  send(params: { to: string; subject: string; body: string }): Promise<void>;
}

export const consoleEmailSender: EmailSender = {
  async send(params) {
    // eslint-disable-next-line no-console
    console.log(`[email:placeholder] to=${params.to} subject="${params.subject}"`);
  },
};
