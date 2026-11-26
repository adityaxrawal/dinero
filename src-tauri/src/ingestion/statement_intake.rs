//! Hands a statement email's PDF attachments to the Statement Queue.
//!
//! One copy shared by live polling and historical scans (audit A-13). Any
//! failure is recorded in `scan_failed_messages` so "Retry failed" can see it,
//! rather than only logged while the cursor moves past the email (audit A-4).
use crate::ingestion::gmail_client::GmailClient;
use crate::ingestion::message_processor::EmailMetadata;
use crate::ingestion::mime_sanitization::PdfAttachmentMeta;
use deadpool_sqlite::Pool;
use tauri::{Manager, Runtime};

/// Stages each attachment and queues it for parsing; failures are recorded
/// for retry, one row per failed attachment.
pub async fn intake_email_statement<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    client: &GmailClient,
    account_id: &str,
    msg_id: &str,
    attachments: &[PdfAttachmentMeta],
    email_meta: Option<EmailMetadata>,
) {
    for att in attachments {
        let staged = intake_attachment(app, pool, client, msg_id, att, email_meta.clone()).await;
        if let Err(e) = staged {
            tracing::error!(
                "Statement attachment '{}' on msg_id='{}' failed, recorded for retry: {:#}",
                att.filename,
                msg_id,
                e
            );
            record_failure(pool, account_id, msg_id, format!("statement attachment: {e:#}")).await;
        }
    }
}

async fn intake_attachment<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    client: &GmailClient,
    msg_id: &str,
    att: &PdfAttachmentMeta,
    email_meta: Option<EmailMetadata>,
) -> anyhow::Result<()> {
    let filename = &att.filename;
    let pdf_bytes = match (&att.inline_bytes, &att.attachment_id) {
        (Some(bytes), _) => bytes.clone(),
        (None, Some(att_id)) => client.fetch_attachment(msg_id, att_id).await?,
        (None, None) => return Ok(()),
    };
    // None means not a valid PDF or already imported: a deliberate skip.
    let Some(file_hash) = crate::statements::duplicate_check::hash_email_attachment_if_new(
        &pdf_bytes, filename, msg_id, pool,
    )
    .await
    else {
        return Ok(());
    };

    let stmt_id = uuid::Uuid::new_v4().to_string();
    let issuer_hint = email_meta.as_ref().and_then(|m| m.bank_name.clone());
    let password = match crate::statements::password::resolve_statement_password(
        &stmt_id, &pdf_bytes, filename, msg_id, pool, app, email_meta,
    )
    .await?
    {
        crate::statements::password::StatementPasswordResolution::Proceed(password) => password,
        // Parked as awaiting-password, with the PDF kept: handled, not lost.
        crate::statements::password::StatementPasswordResolution::PromptCreated => return Ok(()),
    };

    // The PDF is staged before the row: a `statements` row is what marks the
    // email processed, so it must never exist without its PDF.
    let app_dir = app.path().app_data_dir()?;
    crate::statements::pdf_storage::store_pdf(&app_dir, &stmt_id, &pdf_bytes)?;
    drop(pdf_bytes);
    let row = {
        let (id, msg, name) = (stmt_id.clone(), msg_id.to_string(), filename.clone());
        pool.get()
            .await
            .map_err(|e| anyhow::anyhow!("pool error: {e}"))?
            .interact(move |c| {
                crate::db::statements::insert_queued(c, &id, "gmail_email", Some(&msg), None)?;
                crate::db::statements::set_source_filename(c, &id, &name)
            })
            .await
            .map_err(|e| anyhow::anyhow!("interact error: {e}"))
            .and_then(|r| r)
    };
    if let Err(e) = row {
        let _ = crate::statements::pdf_storage::delete_pdf(&app_dir, &stmt_id);
        return Err(e);
    }

    let job = crate::ingestion::queues::StatementJob {
        filename: filename.clone(),
        file_hash,
        stmt_id,
        batch_progress: None,
        password,
        origin: "email_scan".to_string(),
        issuer_hint,
    };
    let st_tx = app.state::<crate::ingestion::queues::QueueHandles>().statement_tx.clone();
    if st_tx.send(job).await.is_err() {
        // The queued row and its PDF are recovered at the next launch.
        tracing::error!("Statement Queue closed — msg_id='{}' file='{}' left for launch recovery", msg_id, filename);
    }
    Ok(())
}

async fn record_failure(pool: &Pool, account_id: &str, msg_id: &str, error: String) {
    let row = crate::db::scan_failed_messages::ScanFailedMessageRow {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        msg_id: msg_id.to_string(),
        error,
        failed_at: None,
    };
    let written = match pool.get().await {
        Ok(conn) => conn
            .interact(move |c| crate::db::scan_failed_messages::insert(c, &row))
            .await
            .map_err(|e| anyhow::anyhow!("interact error: {e}"))
            .and_then(|r| r),
        Err(e) => Err(anyhow::anyhow!("pool error: {e}")),
    };
    if let Err(e) = written {
        tracing::error!("Could not record failed statement email msg_id='{}': {:#}", msg_id, e);
    }
}

#[cfg(test)]
#[path = "statement_intake_tests.rs"]
mod tests;
