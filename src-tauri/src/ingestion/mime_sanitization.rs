use crate::ingestion::gmail_client::MessagePart;
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use regex::Regex;

/// Doc 30 TASK-GMAIL-002: attachment metadata (filename, mimeType, attachmentId, size)
/// captured from the message payload alone — no attachment bytes are fetched here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfAttachmentMeta {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedMessage {
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    /// True if at least one PDF attachment is present (convenience flag).
    pub has_pdf_attachment: bool,
    /// All PDF attachments found in the MIME tree, metadata only.
    /// This is empty if the PDF data was inline (rare) — in that case has_pdf_attachment
    /// may still be true but there is nothing to fetch separately.
    pub pdf_attachments: Vec<PdfAttachmentMeta>,
}

pub fn extract_body_and_attachments(part: &MessagePart) -> ExtractedMessage {
    let mut extracted = ExtractedMessage {
        text_body: None,
        html_body: None,
        has_pdf_attachment: false,
        pdf_attachments: Vec::new(),
    };
    extract_recursive(part, &mut extracted);

    // If we only have HTML, sanitize it and set it as text_body
    if let (None, Some(html)) = (&extracted.text_body, &extracted.html_body) {
        extracted.text_body = Some(sanitize_html(html));
    }

    if let Some(ref text) = extracted.text_body {
        extracted.text_body = Some(sanitize_plain_text(text));
    }

    extracted
}

fn extract_recursive(part: &MessagePart, extracted: &mut ExtractedMessage) {
    if part.mime_type == "text/plain" {
        if extracted.text_body.is_none() {
            if let Some(body) = &part.body {
                if let Some(data) = &body.data {
                    if let Ok(decoded) = URL_SAFE.decode(data) {
                        extracted.text_body = String::from_utf8(decoded).ok();
                    }
                }
            }
        }
    } else if part.mime_type == "text/html" {
        if extracted.html_body.is_none() {
            if let Some(body) = &part.body {
                if let Some(data) = &body.data {
                    if let Ok(decoded) = URL_SAFE.decode(data) {
                        extracted.html_body = String::from_utf8(decoded).ok();
                    }
                }
            }
        }
    } else if part.mime_type == "application/pdf" {
        extracted.has_pdf_attachment = true;
        // Collect attachment metadata so the caller can download the bytes later.
        if let Some(body) = &part.body {
            if let Some(ref att_id) = body.attachment_id {
                let filename = part
                    .filename
                    .clone()
                    .unwrap_or_else(|| "statement.pdf".to_string());
                extracted.pdf_attachments.push(PdfAttachmentMeta {
                    attachment_id: att_id.clone(),
                    filename,
                    mime_type: part.mime_type.clone(),
                    size: body.size,
                });
            }
        }
    } else if let Some(filename) = &part.filename {
        if filename.to_lowercase().ends_with(".pdf") {
            extracted.has_pdf_attachment = true;
            // Attempt to collect attachment metadata for non-explicit-mimetype PDFs.
            if let Some(body) = &part.body {
                if let Some(ref att_id) = body.attachment_id {
                    extracted.pdf_attachments.push(PdfAttachmentMeta {
                        attachment_id: att_id.clone(),
                        filename: filename.clone(),
                        mime_type: part.mime_type.clone(),
                        size: body.size,
                    });
                }
            }
        }
    }

    if let Some(parts) = &part.parts {
        for sub_part in parts {
            extract_recursive(sub_part, extracted);
        }
    }
}

pub fn sanitize_plain_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut sanitized = Vec::new();

    for line in lines {
        // Strip quoted replies
        if line.trim_start().starts_with('>') {
            continue;
        }
        // Strip signature block, delimited per convention by a lone "--" line
        // (trailing whitespace is already gone after .trim()).
        if line.trim() == "--" {
            break;
        }
        sanitized.push(line.trim_end());
    }

    let mut result = sanitized.join("\n");
    // Normalize whitespace (more than 2 newlines -> 2 newlines)
    let re = Regex::new(r"\n{3,}").unwrap();
    result = re.replace_all(&result, "\n\n").to_string();

    result.trim().to_string()
}

pub fn sanitize_html(html: &str) -> String {
    // Basic HTML sanitization:
    // 1. Remove <script> and <style> tags and their contents
    // 2. Convert common block tags to newlines (e.g. <div>, <p>, <br>)
    // 3. Remove all other HTML tags

    let mut text = html.to_string();

    let script_re = Regex::new(r"(?is)<script.*?>.*?</script>").unwrap();
    text = script_re.replace_all(&text, "").to_string();

    let style_re = Regex::new(r"(?is)<style.*?>.*?</style>").unwrap();
    text = style_re.replace_all(&text, "").to_string();

    let block_re = Regex::new(r"(?i)<(div|p|br|li|tr|table|h1|h2|h3|h4|h5|h6)[^>]*>").unwrap();
    text = block_re.replace_all(&text, "\n").to_string();

    let tag_re = Regex::new(r"(?is)<.*?>").unwrap();
    text = tag_re.replace_all(&text, "").to_string();

    // Decode HTML entities (simplified)
    text = text.replace("&nbsp;", " ");
    text = text.replace("&amp;", "&");
    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&quot;", "\"");
    text = text.replace("&#39;", "'");

    sanitize_plain_text(&text)
}
