use crate::ingestion::gmail_client::{MessagePart, MessagePartBody};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use regex::Regex;

/// Doc 30 TASK-GMAIL-002: attachment metadata (filename, mimeType, attachmentId, size)
/// captured from the message payload alone — no attachment bytes are fetched here,
/// except `inline_bytes` below, which Gmail already handed us in the payload itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfAttachmentMeta {
    /// `Some` when the bytes must be fetched separately via `attachments.get`
    /// (the common case for real attachments). `None` when Gmail inlined the
    /// bytes directly (`inline_bytes` below) instead of assigning an ID —
    /// exactly one of the two is always `Some` for any entry in
    /// `pdf_attachments` (see `extract_recursive`'s push condition).
    pub attachment_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub size: Option<i32>,
    /// Raw bytes when Gmail inlined the attachment directly in the message
    /// payload (`body.data`) rather than requiring a separate
    /// `attachments.get` fetch. Decoded eagerly here since the base64 is
    /// already in hand — no reason to make the caller re-derive it.
    ///
    /// Real bug this replaced: previously this case (`body.attachmentId`
    /// absent, `body.data` present) was silently dropped entirely —
    /// `has_pdf_attachment` was still set `true` but nothing was ever pushed
    /// here, so the caller saw an empty `pdf_attachments` and skipped the
    /// statement outright with just a WARN log (field error log: "has
    /// has_pdf_attachment=true but no downloadable attachment_ids").
    pub inline_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedMessage {
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    /// True if at least one PDF attachment is present (convenience flag).
    pub has_pdf_attachment: bool,
    /// All PDF attachments found in the MIME tree, whose bytes are
    /// obtainable one way or another (see `PdfAttachmentMeta`). Empty only
    /// when a PDF-typed part had neither an `attachmentId` nor inline
    /// `data` — a genuinely malformed/unexpected payload, not the inline-data
    /// case this struct used to conflate with that.
    pub pdf_attachments: Vec<PdfAttachmentMeta>,
    /// Structural (never content) diagnostics for each PDF-shaped part that
    /// `pdf_attachments` couldn't collect bytes for — part_id, mime_type,
    /// and which of {body, attachmentId, data} were present. Empty in the
    /// overwhelmingly common case; exists so a "has_pdf_attachment=true but
    /// no downloadable attachment_ids" skip is diagnosable from the log
    /// alone instead of unreproducible without the original email.
    pub skipped_pdf_parts: Vec<String>,
}

pub fn extract_body_and_attachments(part: &MessagePart) -> ExtractedMessage {
    let mut extracted = ExtractedMessage {
        text_body: None,
        html_body: None,
        has_pdf_attachment: false,
        pdf_attachments: Vec::new(),
        skipped_pdf_parts: Vec::new(),
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
        // Collect attachment metadata so the caller can obtain the bytes
        // later. A completely absent `body` (not just an empty one) used to
        // silently skip this call, giving `has_pdf_attachment=true` with
        // nothing pushed via a second, undocumented path -- collapse it into
        // the single documented "no attachmentId, no data" case in
        // `push_pdf_attachment` instead, so there's exactly one place that
        // decides "this PDF part is unusable" and one log for it.
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
            // Attempt to collect attachment metadata for non-explicit-mimetype PDFs.
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

/// Pushes a `PdfAttachmentMeta` when `body` gives us a way to obtain its
/// bytes — either an `attachmentId` to fetch separately, or `data` Gmail
/// already inlined. Pushes nothing (leaving `has_pdf_attachment` true but
/// `pdf_attachments` short one entry) only when neither is present, which
/// is a genuinely malformed/unexpected payload.
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

    // Strip any bare CSS rule blocks left un-enclosed
    let loose_css_re = Regex::new(r"(?i)(?:@media[^{]+\{[\s\S]*?\}|body|table|td|th|p|a|img|\.[a-z0-9_-]+|#[a-z0-9_-]+)\s*\{[^}]*\}").unwrap();
    text = loose_css_re.replace_all(&text, "").to_string();

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

/// Sanitizes the *original* HTML body for visual display (password-prompt
/// modal's "show the email like Gmail does" view) -- unlike `sanitize_html`
/// above, which reduces everything to plain text, this keeps layout/typography
/// markup (tables, inline `style`, headings, etc.) so the rendered result
/// still looks like the source email instead of being reflowed prose.
///
/// Beyond ammonia's own script/event-handler/dangerous-URL-scheme stripping,
/// one deliberate restriction:
/// - Any CSS `url(...)` reference (inline `style="background:url(...)"` or
///   a `<style>` block) is neutralized first, since ammonia passes an
///   allowed attribute's *value* through unparsed, making a background-image
///   a tracking-pixel vector.
///
/// **`<img>` and `<a href>` are allowed through** (`09d0351`, "improve MIME
/// sanitization for complex HTML structures" — pinned by
/// `test_sanitize_html_for_display_preserves_img_and_links`), which is what
/// makes a bank email render like it does in Gmail. The cost is that a remote
/// `<img src>` *does* load when the viewer opens the email, so a tracking
/// pixel fires — the one network call this otherwise offline-first app makes
/// on a render. Blocking it again is a one-line `.rm_tags(["img"])`, at the
/// price of logo-less, layout-broken emails.
///
/// This comment used to claim both tags were dropped, describing the
/// `rm_tags(["img", "a"])` that `09d0351` replaced. audit_08 #9 flagged the
/// wrong half of this: inline `data:` URIs are inert (they cannot phone
/// home); remote `src` is the actual egress.
///
/// Caller renders the result inside `<iframe srcDoc sandbox="allow-same-origin
/// allow-popups">` — no `allow-scripts` — so this sanitizer is a second,
/// independent layer, not the only one.
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
