//! Safely extracts body text and attachments from a MIME message.
//!
//! A trust boundary. Bank emails are HTML from an external party, so the display
//! sanitiser strips scripting and active content before anything is rendered,
//! and the extraction path reduces the message to plain text so parsing operates
//! on content rather than markup.
use crate::ingestion::gmail_client::{MessagePart, MessagePartBody};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfAttachmentMeta {
    pub attachment_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub size: Option<i32>,
    pub inline_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedMessage {
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub has_pdf_attachment: bool,
    pub pdf_attachments: Vec<PdfAttachmentMeta>,
    pub skipped_pdf_parts: Vec<String>,
}

/// Extracts body text and PDF attachments from a MIME message.
pub fn extract_body_and_attachments(part: &MessagePart) -> ExtractedMessage {
    let mut extracted = ExtractedMessage {
        text_body: None,
        html_body: None,
        has_pdf_attachment: false,
        pdf_attachments: Vec::new(),
        skipped_pdf_parts: Vec::new(),
    };
    extract_recursive(part, &mut extracted);

    if let (None, Some(html)) = (&extracted.text_body, &extracted.html_body) {
        extracted.text_body = Some(sanitize_html(html));
    }

    if let Some(ref text) = extracted.text_body {
        extracted.text_body = Some(sanitize_plain_text(text));
    }

    extracted
}

/// Walks the MIME tree, collecting parts.
///
/// Recursive because MIME nests arbitrarily -- a multipart/mixed wrapping a
/// multipart/alternative is the normal shape of a bank email with an attachment.
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
        let filename = part
            .filename
            .clone()
            .unwrap_or_else(|| "statement.pdf".to_string());
        let empty_body = MessagePartBody {
            size: None,
            data: None,
            attachment_id: None,
        };
        let body = part.body.as_ref().unwrap_or(&empty_body);
        push_pdf_attachment(
            extracted,
            body,
            filename,
            part.mime_type.clone(),
            part.part_id.as_deref(),
            part.body.is_some(),
        );
    } else if let Some(filename) = &part.filename {
        if filename.to_lowercase().ends_with(".pdf") {
            extracted.has_pdf_attachment = true;
            let empty_body = MessagePartBody {
                size: None,
                data: None,
                attachment_id: None,
            };
            let body = part.body.as_ref().unwrap_or(&empty_body);
            push_pdf_attachment(
                extracted,
                body,
                filename.clone(),
                part.mime_type.clone(),
                part.part_id.as_deref(),
                part.body.is_some(),
            );
        }
    }

    if let Some(parts) = &part.parts {
        for sub_part in parts {
            extract_recursive(sub_part, extracted);
        }
    }
}

/// Records a PDF attachment's metadata for later retrieval.
fn push_pdf_attachment(
    extracted: &mut ExtractedMessage,
    body: &MessagePartBody,
    filename: String,
    mime_type: String,
    part_id: Option<&str>,
    body_present: bool,
) {
    let inline_bytes = body
        .data
        .as_ref()
        .and_then(|data| URL_SAFE.decode(data).ok());
    if body.attachment_id.is_none() && inline_bytes.is_none() {
        extracted.skipped_pdf_parts.push(format!(
            "part_id={} mime={} body_present={} attachment_id_present=false data_present=false",
            part_id.unwrap_or("?"),
            mime_type,
            body_present,
        ));
        return;
    }
    extracted.pdf_attachments.push(PdfAttachmentMeta {
        attachment_id: body.attachment_id.clone(),
        filename,
        mime_type,
        size: body.size,
        inline_bytes,
    });
}

/// Normalises plain text for parsing.
pub fn sanitize_plain_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut sanitized = Vec::new();

    for line in lines {
        if line.trim_start().starts_with('>') {
            continue;
        }
        if line.trim() == "--" {
            break;
        }
        sanitized.push(line.trim_end());
    }

    let mut result = sanitized.join("\n");
    let re = Regex::new(r"\n{3,}").unwrap();
    result = re.replace_all(&result, "\n\n").to_string();

    result.trim().to_string()
}

/// Strips markup from HTML, leaving text for extraction.
pub fn sanitize_html(html: &str) -> String {

    let mut text = html.to_string();

    let script_re = Regex::new(r"(?is)<script.*?>.*?</script>").unwrap();
    text = script_re.replace_all(&text, "").to_string();

    let style_re = Regex::new(r"(?is)<style.*?>.*?</style>").unwrap();
    text = style_re.replace_all(&text, "").to_string();

    let loose_css_re = Regex::new(r"(?i)(?:@media[^{]+\{[\s\S]*?\}|body|table|td|th|p|a|img|\.[a-z0-9_-]+|#[a-z0-9_-]+)\s*\{[^}]*\}").unwrap();
    text = loose_css_re.replace_all(&text, "").to_string();

    let block_re = Regex::new(r"(?i)<(div|p|br|li|tr|table|h1|h2|h3|h4|h5|h6)[^>]*>").unwrap();
    text = block_re.replace_all(&text, "\n").to_string();

    let tag_re = Regex::new(r"(?is)<.*?>").unwrap();
    text = tag_re.replace_all(&text, "").to_string();

    text = text.replace("&nbsp;", " ");
    text = text.replace("&amp;", "&");
    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&quot;", "\"");
    text = text.replace("&#39;", "'");

    sanitize_plain_text(&text)
}

/// Sanitises HTML for safe rendering in the viewer.
///
/// Distinct from the extraction path above: this one keeps the markup so the
/// email still looks like itself, but removes scripting and active content. The
/// input is untrusted third-party HTML, so this is a trust boundary.
pub fn sanitize_html_for_display(html: &str) -> String {
    let css_url_re = Regex::new(r"(?i)url\s*\([^)]*\)").unwrap();
    let defanged = css_url_re.replace_all(html, "none").to_string();

    ammonia::Builder::default()
        .rm_clean_content_tags(["style"])
        .add_tags([
            "style",
            "img",
            "a",
            "div",
            "span",
            "p",
            "b",
            "i",
            "strong",
            "em",
            "u",
            "s",
            "sub",
            "sup",
            "font",
            "center",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "table",
            "tbody",
            "thead",
            "tfoot",
            "tr",
            "td",
            "th",
            "caption",
            "colgroup",
            "col",
            "ul",
            "ol",
            "li",
            "dl",
            "dt",
            "dd",
            "br",
            "hr",
            "blockquote",
            "pre",
            "code",
            "section",
            "article",
            "header",
            "footer",
            "main",
            "nav",
            "aside",
            "head",
            "body",
            "html",
            "title",
            "meta",
            "link",
        ])
        .add_generic_attributes([
            "style",
            "class",
            "id",
            "width",
            "height",
            "align",
            "valign",
            "bgcolor",
            "color",
            "face",
            "size",
            "cellpadding",
            "cellspacing",
            "border",
            "colspan",
            "rowspan",
            "dir",
            "lang",
            "alt",
            "title",
            "target",
            "type",
            "media",
            "href",
            "src",
            "srcset",
        ])
        .clean(&defanged)
        .to_string()
}
