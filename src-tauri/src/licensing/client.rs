use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::network_client::NetworkClient;

/// Doc 19 §14.2: activation is a Razorpay payment confirmation bound to this
/// device — there is no separate "license_key" concept in the documented model.
#[derive(Serialize)]
pub struct ActivateRequest {
    pub email: String,
    pub razorpay_payment_id: String,
    pub razorpay_signature: String,
    pub device_id: String,
    pub billing_interval: String,
}

#[derive(Debug, Deserialize)]
pub struct LicenseResponse {
    pub jwt: String,
    pub status: String,
}

/// Document 19 §3.4/§4's error response shape (`{code, message, details}`),
/// used by the Licensing Backend's HTTP error responses same as the local
/// IPC error contract.
#[derive(Deserialize)]
struct LicensingErrorResponse {
    code: String,
    #[allow(dead_code)]
    message: Option<String>,
}

/// Doc 19 §14.2/§14.4: post-activation revalidation is keyed on `device_id`
/// alone — one device is bound to exactly one license (§11.3), so there is no
/// separate license key to carry.
#[derive(Serialize)]
pub struct ValidateRequest {
    pub device_id: String,
}

pub struct LicensingClient {
    network: NetworkClient,
    base_url: String,
}

impl LicensingClient {
    /// Doc 01 §10.4 (BG-02): every Licensing Backend call must route through
    /// `NetworkClient` so it's captured in the local Network Activity audit
    /// trail — this used to build its own bare `reqwest::Client`, making
    /// every licensing call invisible to that log.
    pub fn new(base_url: String, db_pool: deadpool_sqlite::Pool) -> Self {
        Self {
            network: NetworkClient::new(db_pool),
            base_url,
        }
    }

    pub async fn activate(&self, req: ActivateRequest) -> Result<LicenseResponse> {
        let url = format!("{}/api/license/activate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;

        // Document 19 §14.2/Document 30 TASK-AUTH-011: `DEVICE_ALREADY_BOUND`
        // must be surfaced clearly, with guidance to deactivate elsewhere
        // first — not flattened into a generic network-error string, which
        // `error_for_status_ref()` alone would do (it only sees the HTTP
        // status, never the error body carrying the actual code).
        if !res.status().is_success() {
            if let Ok(body) = res.json::<LicensingErrorResponse>().await {
                anyhow::bail!(body.code);
            }
            anyhow::bail!("Licensing Backend activation request failed");
        }

        let data = res.json::<LicenseResponse>().await?;
        Ok(data)
    }

    pub async fn validate(&self, req: ValidateRequest) -> Result<LicenseResponse> {
        let url = format!("{}/api/license/validate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;
        res.error_for_status_ref()?;
        let data = res.json::<LicenseResponse>().await?;
        Ok(data)
    }

    pub async fn deactivate(&self, req: ValidateRequest) -> Result<()> {
        let url = format!("{}/api/license/deactivate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;
        res.error_for_status_ref()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `test_licensing_backend_receives_no_financial_data` (Document 30
    /// TASK-AUTH-010) for the periodic-validation call specifically:
    /// `ValidateRequest`'s payload must be exactly `{device_id}` — no Gmail
    /// tokens, financial data, transaction counts, or instrument details,
    /// and (per Document 19 §14.2/§14.4's device_id-only revalidation
    /// design) not even `license_key`/`email` either, since this system has
    /// no separate license-key concept post-activation.
    #[tokio::test]
    async fn test_licensing_backend_receives_no_financial_data_on_validate() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/license/validate")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "device_id": "some-device-id"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jwt":"mock_jwt","status":"active"}"#)
            .create_async()
            .await;

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let client = LicensingClient::new(server.url(), pool);
        let res = client
            .validate(ValidateRequest {
                device_id: "some-device-id".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(res.status, "active");
        mock.assert_async().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// TASK-AUTH-011: `DEVICE_ALREADY_BOUND` must be surfaced clearly as
    /// that exact code, not flattened into a generic error string that
    /// loses which specific failure occurred.
    #[tokio::test]
    async fn activate_surfaces_device_already_bound_distinctly() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/license/activate")
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":"DEVICE_ALREADY_BOUND","message":"License already bound to another device"}"#)
            .create_async()
            .await;

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let client = LicensingClient::new(server.url(), pool);
        let err = client
            .activate(ActivateRequest {
                email: "test@example.com".to_string(),
                razorpay_payment_id: "pay_1".to_string(),
                razorpay_signature: "sig".to_string(),
                device_id: "some-device".to_string(),
                billing_interval: "monthly".to_string(),
            })
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "DEVICE_ALREADY_BOUND");
        mock.assert_async().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
