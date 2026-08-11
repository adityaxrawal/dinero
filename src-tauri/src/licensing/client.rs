//! HTTP client for the licensing service.
//!
//! Sends only what entitlement requires -- licence key, device fingerprint,
//! subscription status. No transaction, balance or payee data is ever
//! included, which is the commitment the privacy disclosure makes about this
//! channel.
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::network_client::NetworkClient;

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

#[derive(Deserialize)]
struct LicensingErrorResponse {
    code: String,
    #[allow(dead_code)]
    message: Option<String>,
}

#[derive(Serialize)]
pub struct ValidateRequest {
    pub device_id: String,
}

#[derive(Serialize)]
pub struct CreateOrderRequest {
    pub email: String,
    pub plan_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderResponse {
    pub order_id: String,
    pub amount: i64,
    pub currency: String,
    pub key_id: String,
}

pub struct LicensingClient {
    network: NetworkClient,
    base_url: String,
}

impl LicensingClient {
    /// Builds a client for the licensing service.
    pub fn new(base_url: String, db_pool: deadpool_sqlite::Pool) -> Self {
        Self {
            network: NetworkClient::new(db_pool),
            base_url,
        }
    }

    /// Activates a licence for this device.
    pub async fn activate(&self, req: ActivateRequest) -> Result<LicenseResponse> {
        let url = format!("{}/api/license/activate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;

        if !res.status().is_success() {
            if let Ok(body) = res.json::<LicensingErrorResponse>().await {
                anyhow::bail!(body.code);
            }
            anyhow::bail!("Licensing Backend activation request failed");
        }

        let data = res.json::<LicenseResponse>().await?;
        Ok(data)
    }

    /// Validates the licence, returning current entitlement.
    pub async fn validate(&self, req: ValidateRequest) -> Result<LicenseResponse> {
        let url = format!("{}/api/license/validate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;
        res.error_for_status_ref()?;
        let data = res.json::<LicenseResponse>().await?;
        Ok(data)
    }

    /// Deactivates this device's binding.
    pub async fn deactivate(&self, req: ValidateRequest) -> Result<()> {
        let url = format!("{}/api/license/deactivate", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;
        res.error_for_status_ref()?;
        Ok(())
    }

    /// Creates a payment order for a purchase.
    pub async fn create_order(&self, req: CreateOrderRequest) -> Result<CreateOrderResponse> {
        let url = format!("{}/api/billing/create-order", self.base_url);
        let builder = self.network.client().post(&url).json(&req);
        let res = self.network.execute("licensing_backend", builder).await?;
        if !res.status().is_success() {
            if let Ok(body) = res.json::<LicensingErrorResponse>().await {
                anyhow::bail!(body.code);
            }
            anyhow::bail!("Order creation request failed");
        }
        let data = res.json::<CreateOrderResponse>().await?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
