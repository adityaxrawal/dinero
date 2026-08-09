// Doc 30 TASK-BILL-002: Razorpay Checkout Integration (Desktop-Initiated).
// Mirrors `ingestion::oauth`'s loopback-listener pattern exactly (a local
// URL scheme was considered and rejected there, Doc 22 §5 -- the same
// reasoning applies here, so this reuses the identical mechanism rather
// than inventing a second one).
use anyhow::{Context, Result};
use tiny_http::{Header, Response, Server};

use crate::ingestion::oauth::oauth_result_page;
use crate::licensing::client::CreateOrderResponse;

/// Doc 30 TASK-BILL-002: "Abandoned checkout (browser closed mid-flow) uses
/// the same 5-minute timeout pattern as OAuth."
pub const CHECKOUT_CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Serves Razorpay's hosted checkout widget, configured to redirect back to
/// this same loopback server on success/failure/dismissal -- never embeds
/// raw card-entry fields in the Tauri WebView itself (PCI-DSS scope stays
/// entirely with Razorpay).
fn checkout_page(order: &CreateOrderResponse, redirect_port: u16) -> String {
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Dinero — Checkout</title>
<script src="https://checkout.razorpay.com/v1/checkout.js"></script></head>
<body>
<script>
  var options = {{
    key: "{key_id}",
    order_id: "{order_id}",
    amount: {amount},
    currency: "{currency}",
    name: "Dinero",
    handler: function (response) {{
      window.location.href = "http://127.0.0.1:{port}/?razorpay_payment_id=" +
        encodeURIComponent(response.razorpay_payment_id) +
        "&razorpay_signature=" + encodeURIComponent(response.razorpay_signature);
    }},
    modal: {{
      ondismiss: function () {{
        window.location.href = "http://127.0.0.1:{port}/?dismissed=1";
      }}
    }}
  }};
  var rzp = new Razorpay(options);
  rzp.open();
</script>
</body></html>"#,
        key_id = order.key_id,
        order_id = order.order_id,
        amount = order.amount,
        currency = order.currency,
        port = redirect_port,
    )
}

#[derive(Debug)]
pub struct CheckoutResult {
    pub razorpay_payment_id: String,
    pub razorpay_signature: String,
}

/// Blocks (call from `spawn_blocking`) serving the checkout page then
/// waiting for either a successful-payment redirect or a dismissal,
/// bounded by `timeout` -- structurally identical to
/// `ingestion::oauth::wait_for_oauth_callback`.
pub fn serve_checkout_and_wait(
    server: &Server,
    order: &CreateOrderResponse,
    timeout: std::time::Duration,
) -> Result<CheckoutResult> {
    let redirect_port = server
        .server_addr()
        .to_ip()
        .context("Loopback server has no IP address")?
        .port();
    let page = checkout_page(order, redirect_port);
    let deadline = std::time::Instant::now() + timeout;

    loop {
        let remaining = match deadline.checked_duration_since(std::time::Instant::now()) {
            Some(d) if !d.is_zero() => d,
            _ => anyhow::bail!("checkout_timeout"),
        };
        match server.recv_timeout(remaining) {
            Ok(Some(request)) => {
                let url = request.url().to_string();
                let html_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                        .expect("static header is valid");

                if url == "/" || url.starts_with("/?checkout") {
                    // Initial page load -- serve the Razorpay widget itself.
                    let response = Response::from_string(page.clone()).with_header(html_header);
                    let _ = request.respond(response);
                    continue;
                }

                if url.starts_with("/?razorpay_payment_id=") {
                    let parsed = url::Url::parse(&format!("http://localhost{}", url)).unwrap();
                    let mut payment_id = None;
                    let mut signature = None;
                    for (k, v) in parsed.query_pairs() {
                        if k == "razorpay_payment_id" {
                            payment_id = Some(v.into_owned());
                        } else if k == "razorpay_signature" {
                            signature = Some(v.into_owned());
                        }
                    }
                    let response =
                        Response::from_string(oauth_result_page(true, "")).with_header(html_header);
                    let _ = request.respond(response);
                    return match (payment_id, signature) {
                        (Some(p), Some(s)) => Ok(CheckoutResult {
                            razorpay_payment_id: p,
                            razorpay_signature: s,
                        }),
                        _ => anyhow::bail!("checkout_malformed_redirect"),
                    };
                }

                if url.starts_with("/?dismissed") {
                    let response =
                        Response::from_string(oauth_result_page(false, "Checkout cancelled"))
                            .with_header(html_header)
                            .with_status_code(400);
                    let _ = request.respond(response);
                    anyhow::bail!("checkout_dismissed");
                }

                // Unrelated request (favicon probe, etc.) -- keep waiting.
                let _ = request.respond(Response::from_string("").with_status_code(404));
            }
            Ok(None) => anyhow::bail!("checkout_timeout"),
            Err(e) => anyhow::bail!("Checkout callback server error: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_order() -> CreateOrderResponse {
        CreateOrderResponse {
            order_id: "order_abc".to_string(),
            amount: 29900,
            currency: "INR".to_string(),
            key_id: "rzp_test_key".to_string(),
        }
    }

    /// Doc 30 TASK-BILL-002: "Abandoned checkout (browser closed mid-flow)
    /// uses the same 5-minute timeout pattern as OAuth." Uses a short
    /// injected timeout (rather than the real 5 minutes) so the test stays
    /// fast -- mirrors `oauth::oauth_callback_wait_times_out_when_browser_never_completes`.
    #[test]
    fn test_checkout_timeout_after_5_minutes() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let result = serve_checkout_and_wait(
            &server,
            &fake_order(),
            std::time::Duration::from_millis(200),
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "checkout_timeout");
    }

    #[test]
    fn test_checkout_page_embeds_order_details() {
        let page = checkout_page(&fake_order(), 54321);
        assert!(page.contains("order_abc"));
        assert!(page.contains("rzp_test_key"));
        assert!(page.contains("127.0.0.1:54321"));
    }
}
