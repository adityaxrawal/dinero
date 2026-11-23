# Licensing backend: deploying the system-review changes

These steps apply to the changes on branch `fix/system-review` (review findings
C4, H1–H3, M15, M16 and their follow-ups). Do them in order.

## 1. Database schema (Prisma)

Two schema changes need pushing before the new code runs:

- `payment_provider_records.razorpay_payment_id` becomes `UNIQUE` (H1).
- New table `processed_webhook_events` (H3).

Before pushing, check that no payment id is already used twice, or the unique
constraint will fail:

```sql
SELECT razorpay_payment_id, COUNT(*) FROM payment_provider_records
WHERE razorpay_payment_id IS NOT NULL
GROUP BY razorpay_payment_id HAVING COUNT(*) > 1;
```

Resolve any duplicates (they are exactly the replayed activations H1 closes),
then run `npx prisma db push` (this project has no migration history).

## 2. Environment variables

| Variable | Needed by | Notes |
|---|---|---|
| `JWT_PUBLIC_KEY_PEM` | validate, deactivate, refresh-token | Now required by validate and deactivate to check the device token (M16). |
| `JWT_PRIVATE_KEY_PEM` | all signing endpoints | Unchanged. |
| `RAZORPAY_KEY_ID` / `RAZORPAY_KEY_SECRET` | activate | activate now fetches the payment to check status and amount (H1). |
| `RAZORPAY_WEBHOOK_SECRET` | webhook | Unchanged; the HMAC is now over the raw body (H3). |
| `LEGACY_NO_PROOF_UNTIL` | validate, deactivate | Rollout window, see step 3. ISO date, e.g. `2027-01-15T00:00:00Z`. |

Make sure the Razorpay webhook subscription includes `subscription.cancelled`,
`subscription.halted`, `subscription.completed` and `refund.processed`.

## 3. Rollout order

Desktop builds released before this change send no device token to
`validate`/`deactivate`, so the new backend would reject them.

1. Deploy the backend with `LEGACY_NO_PROOF_UNTIL` set a few weeks ahead
   (long enough for users to update). Old builds keep working; requests that
   do send a token are always fully checked.
2. Release the desktop app.
3. After the date passes the window closes on its own. Remove the variable
   when convenient.

Old builds also mishandle the `validated` status (the bug C4 fixes), so
updating the desktop app is required for licences to keep refreshing anyway.

## 4. New endpoint

`POST /api/license/restore` `{ email, razorpay_payment_id, device_id }`
restores a paid licence after a reinstall that lost the local token. It is
rate limited per email and refused while the licence is bound to another
device.
