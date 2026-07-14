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
}
