use super::mime_sanitization::*;
use crate::ingestion::gmail_client::{MessagePart, MessagePartBody};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};

fn create_text_part(text: &str, mime_type: &str) -> MessagePart {
    MessagePart {
        part_id: Some("1".to_string()),
        mime_type: mime_type.to_string(),
        filename: None,
        headers: None,
        body: Some(MessagePartBody {
            size: Some(text.len() as i32),
            data: Some(URL_SAFE.encode(text)),
            attachment_id: None,
        }),
        parts: None,
    }
}

#[test]
fn test_mime_plaintext_preferred_over_html() {
    let plain_text = "This is plain text.";
    let html_text = "<div>This is HTML text.</div>";

    let plain_part = create_text_part(plain_text, "text/plain");
    let html_part = create_text_part(html_text, "text/html");

    let parent_part = MessagePart {
        part_id: None,
        mime_type: "multipart/alternative".to_string(),
        filename: None,
        headers: None,
        body: None,
        parts: Some(vec![html_part, plain_part]),
    };

    let extracted = extract_body_and_attachments(&parent_part);
    assert_eq!(extracted.text_body.unwrap(), "This is plain text.");
    assert!(!extracted.has_pdf_attachment);
}

#[test]
fn test_html_sanitized_preserves_merchant_spans() {
    let html_text = r#"
        <html>
            <head>
                <style>.hidden { display: none; }</style>
                <script>alert('hi');</script>
            </head>
            <body>
                <div>Merchant: <span>Amazon</span></div>
                <br>
                <p>Total: $12.99</p>
            </body>
        </html>
    "#;

    let html_part = create_text_part(html_text, "text/html");

    let parent_part = MessagePart {
        part_id: None,
        mime_type: "multipart/mixed".to_string(),
        filename: None,
        headers: None,
        body: None,
        parts: Some(vec![html_part]),
    };

    let extracted = extract_body_and_attachments(&parent_part);
    let result = extracted.text_body.unwrap();

    // Scripts and styles should be gone.
    assert!(!result.contains("alert"));
    assert!(!result.contains(".hidden"));

    // Content should remain
    assert!(result.contains("Merchant: Amazon"));
    assert!(result.contains("Total: $12.99"));
}

#[test]
fn test_quoted_reply_stripped() {
    let plain_text = "Hello\n\n> This is a quote\n> another line\nMore body text";
    let plain_part = create_text_part(plain_text, "text/plain");
    let extracted = extract_body_and_attachments(&plain_part);
    let result = extracted.text_body.unwrap();
    assert_eq!(result, "Hello\n\nMore body text");
}

#[test]
fn test_signature_block_stripped() {
    let plain_text = "Hello\n\nBody content\n-- \nJohn Doe\n555-1234";
    let plain_part = create_text_part(plain_text, "text/plain");
    let extracted = extract_body_and_attachments(&plain_part);
    let result = extracted.text_body.unwrap();
    assert_eq!(result, "Hello\n\nBody content");
}

#[test]
fn test_pdf_attachment_detection() {
    let pdf_part = MessagePart {
        part_id: Some("2".to_string()),
        mime_type: "application/pdf".to_string(),
        filename: Some("invoice.pdf".to_string()),
        headers: None,
        body: Some(MessagePartBody {
            size: Some(1024),
            data: None,
            attachment_id: Some("att123".to_string()),
        }),
        parts: None,
    };

    let plain_part = create_text_part("See attachment", "text/plain");

    let parent_part = MessagePart {
        part_id: None,
        mime_type: "multipart/mixed".to_string(),
        filename: None,
        headers: None,
        body: None,
        parts: Some(vec![plain_part, pdf_part]),
    };

    let extracted = extract_body_and_attachments(&parent_part);
    assert!(extracted.has_pdf_attachment);
    assert_eq!(extracted.text_body.unwrap(), "See attachment");
    assert_eq!(extracted.pdf_attachments.len(), 1);
    assert_eq!(
        extracted.pdf_attachments[0].attachment_id.as_deref(),
        Some("att123")
    );
    assert_eq!(extracted.pdf_attachments[0].inline_bytes, None);
}

/// Regression test for the field bug report ("StatementEmail ... has
/// has_pdf_attachment=true but no downloadable attachment_ids — skipping
/// parse"): when Gmail inlines the attachment bytes directly (`body.data`,
/// no `attachmentId`) instead of requiring a separate fetch, those bytes
/// must still end up in `pdf_attachments` — previously this case set
/// `has_pdf_attachment` but silently produced zero entries, so the caller
/// saw an empty list and dropped the statement.
#[test]
fn test_pdf_attachment_with_inline_data_is_not_dropped() {
    let pdf_bytes = b"%PDF-1.4 fake pdf bytes";
    let pdf_part = MessagePart {
        part_id: Some("2".to_string()),
        mime_type: "application/pdf".to_string(),
        filename: Some("invoice.pdf".to_string()),
        headers: None,
        body: Some(MessagePartBody {
            size: Some(pdf_bytes.len() as i32),
            data: Some(URL_SAFE.encode(pdf_bytes)),
            attachment_id: None,
        }),
        parts: None,
    };

    let parent_part = MessagePart {
        part_id: None,
        mime_type: "multipart/mixed".to_string(),
        filename: None,
        headers: None,
        body: None,
        parts: Some(vec![pdf_part]),
    };

    let extracted = extract_body_and_attachments(&parent_part);
    assert!(extracted.has_pdf_attachment);
    assert_eq!(
        extracted.pdf_attachments.len(),
        1,
        "inline PDF data must not be silently dropped"
    );
    assert_eq!(extracted.pdf_attachments[0].attachment_id, None);
    assert_eq!(
        extracted.pdf_attachments[0].inline_bytes.as_deref(),
        Some(&pdf_bytes[..])
    );
}

// ── sanitize_html_for_display ────────────────────────────────────────────
//
// Unlike `sanitize_html` (which reduces everything to plain text), this is
// the password-prompt modal's "show the email like Gmail did" renderer --
// it must keep layout/typography markup while still closing off the actual
// attack surface: script execution, tracking pixels (both `<img src>` and
// CSS `url(...)` in a `style` attribute), and clickable links out of a
// password-entry surface.

#[test]
fn test_sanitize_html_for_display_strips_script_and_event_handlers() {
    let html = r#"<div onclick="alert(1)">hi<script>alert('xss')</script></div>"#;
    let out = sanitize_html_for_display(html);
    assert!(!out.contains("<script"), "script tag must be removed: {out}");
    assert!(!out.contains("alert"), "script contents must be removed: {out}");
    assert!(!out.contains("onclick"), "event handler attribute must be stripped: {out}");
    assert!(out.contains("hi"));
}

#[test]
fn test_sanitize_html_for_display_preserves_img_and_links() {
    let html = r#"<p>Statement attached.</p><img src="https://bank.example/logo.png" width="100"><a href="https://bank.example/click">Manage your account</a>"#;
    let out = sanitize_html_for_display(html);
    assert!(out.contains("<img"), "img tag must be preserved: {out}");
    assert!(out.contains("<a"), "anchor tag must be preserved: {out}");
    assert!(out.contains("Manage your account"), "link text should be visible: {out}");
}

#[test]
fn test_sanitize_html_for_display_preserves_style_block() {
    let html = r#"<style>body { margin: 0; padding: 0; } table { border-collapse: collapse; }</style><div>Hello</div>"#;
    let out = sanitize_html_for_display(html);
    assert!(out.contains("<style>"), "<style> tag must be preserved: {out}");
    assert!(out.contains("border-collapse"), "CSS rules inside <style> block must survive: {out}");
}

#[test]
fn test_sanitize_html_for_display_neutralizes_css_background_tracking() {
    let html = r#"<div style="background-image:url(https://tracker.example/pixel.gif);color:red">hi</div>"#;
    let out = sanitize_html_for_display(html);
    assert!(
        !out.contains("tracker.example"),
        "CSS url() tracking reference must be neutralized: {out}"
    );
    assert!(
        out.contains("color:red") || out.contains("color: red"),
        "harmless style properties should survive: {out}"
    );
}

#[test]
fn test_sanitize_html_for_display_preserves_table_layout_and_inline_style() {
    let html = r#"<table><tr><td style="font-weight:bold;color:#c8102e">Total Due</td><td>Rs. 24,560.00</td></tr></table>"#;
    let out = sanitize_html_for_display(html);
    assert!(out.contains("<table"), "table structure must survive: {out}");
    assert!(out.contains("<td"), "table cells must survive: {out}");
    assert!(out.contains("Total Due"));
    assert!(out.contains("style="), "inline style must survive so the email still looks like the original: {out}");
}
