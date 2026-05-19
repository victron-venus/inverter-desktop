use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaEntity {
    pub id: String,
    pub entity_id: String,
    pub label: String,
    pub domain: String,
    pub icon: Option<String>,
    pub order: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaState {
    pub entity_id: String,
    pub state: String,
    pub attributes: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub last_changed: Option<String>,
    #[allow(dead_code)]
    pub last_updated: Option<String>,
}

#[derive(Clone)]
pub struct HaApiClient {
    base_url: String,
    token: String,
    client: Client,
}

impl HaApiClient {
    pub async fn new(url: &str, port: Option<u16>, token: &str) -> Result<Self, String> {
        let host = url.trim_end_matches('/');
        let port = port.unwrap_or(8123);
        let base_url = format!("{}:{}", host, port);

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        Ok(Self {
            base_url,
            token: token.to_string(),
            client,
        })
    }

    pub async fn test_connection(&self) -> Result<(), String> {
        let response = self
            .client
            .get(&format!("{}/api/", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {} - unauthorized or invalid", response.status()))
        }
    }

    pub async fn get_states(&self) -> Result<Vec<HaState>, String> {
        let response = self
            .client
            .get(&format!("{}/api/states", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch states: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {} error", response.status()));
        }

        let states: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let mut result = Vec::new();
        for item in states {
            if let (Some(entity_id), Some(state)) = (item.get("entity_id"), item.get("state")) {
                if let (Ok(entity_id_str), Ok(state_str)) = (
                    serde_json::from_value::<String>(entity_id.clone()),
                    serde_json::from_value::<String>(state.clone()),
                ) {
                    let attributes = item.get("attributes").cloned();
                    let last_changed = item.get("last_changed").and_then(|v| v.as_str().map(String::from));
                    let last_updated = item.get("last_updated").and_then(|v| v.as_str().map(String::from));
                    result.push(HaState {
                        entity_id: entity_id_str,
                        state: state_str,
                        attributes,
                        last_changed,
                        last_updated,
                    });
                }
            }
        }

        Ok(result)
    }

    pub async fn get_entities(&self, domain: &str) -> Result<Vec<serde_json::Value>, String> {
        let states = self.get_states().await?;
        let mut entities = Vec::new();
        for state in states {
            if state.entity_id.starts_with(&format!("{}.", domain)) {
                let mut entity = serde_json::to_value(&state).unwrap_or_default();
                // friendly name from attributes if available
                if let Some(attributes) = &state.attributes {
                    if let Some(friendly) = attributes.get("friendly_name") {
                        if let Some(name) = friendly.as_str() {
                            // inject friendly_name at top level for convenience
                            if let Some(obj) = entity.as_object_mut() {
                                obj.insert("friendly_name".to_string(), serde_json::Value::String(name.to_string()));
                            }
                        }
                    }
                }
                entities.push(entity);
            }
        }
        Ok(entities)
    }

    pub async fn call_service(
        &self,
        entity_id: &str,
        domain: &str,
        service: &str,
        data: serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{}/api/services/{}/{}", self.base_url, domain, service);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "entity_id": entity_id, ..data }))
            .send()
            .await
            .map_err(|e| format!("Service call failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {} error", response.status()))
        }
    }

    pub async fn turn_on(&self, entity_id: &str) -> Result<(), String> {
        let domain = entity_id.split('.').next().unwrap_or("switch");
        self.call_service(entity_id, domain, "turn_on", serde_json::json!({}))
            .await
    }

    pub async fn turn_off(&self, entity_id: &str) -> Result<(), String> {
        let domain = entity_id.split('.').next().unwrap_or("switch");
        self.call_service(entity_id, domain, "turn_off", serde_json::json!({}))
            .await
    }

    pub async fn toggle(&self, entity_id: &str, current_on: bool) -> Result<(), String> {
        if current_on {
            self.turn_off(entity_id).await
        } else {
            self.turn_on(entity_id).await
        }
    }
}