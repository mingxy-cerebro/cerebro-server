use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub status: TenantStatus,
    pub config: TenantConfig,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TenantConfig {
    pub embed_provider: String,
    pub llm_provider: String,
    pub custom: serde_json::Value,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            embed_provider: "noop".to_string(),
            llm_provider: String::new(),
            custom: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthInfo {
    pub tenant_id: String,
    pub agent_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TenantStatus {
    Active,
    Suspended,
    Deleted,
}

impl fmt::Display for TenantStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Suspended => write!(f, "suspended"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

impl FromStr for TenantStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "deleted" => Ok(Self::Deleted),
            _ => Err(format!("unknown tenant status: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_serde_roundtrip() {
        let tenant = Tenant {
            id: "t-001".to_string(),
            name: "test-tenant".to_string(),
            status: TenantStatus::Active,
            config: TenantConfig::default(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&tenant).unwrap();
        let parsed: Tenant = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "t-001");
        assert_eq!(parsed.status, TenantStatus::Active);
    }

    #[test]
    fn auth_info_serde_roundtrip() {
        let auth = AuthInfo {
            tenant_id: "t-001".to_string(),
            agent_id: Some("agent-coder".to_string()),
        };
        let json = serde_json::to_string(&auth).unwrap();
        let parsed: AuthInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tenant_id, "t-001");
        assert_eq!(parsed.agent_id, Some("agent-coder".to_string()));
    }

    #[test]
    fn tenant_status_display_lowercase() {
        assert_eq!(TenantStatus::Active.to_string(), "active");
        assert_eq!(TenantStatus::Suspended.to_string(), "suspended");
        assert_eq!(TenantStatus::Deleted.to_string(), "deleted");
    }

    #[test]
    fn tenant_config_default() {
        let cfg = TenantConfig::default();
        assert_eq!(cfg.embed_provider, "noop");
        assert!(cfg.llm_provider.is_empty());
        assert!(cfg.custom.is_object());
    }
}
