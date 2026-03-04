//! UCP-aligned capability discovery model. Orchestrator-native; maps to UCP
//! without hard dependency on UCP wire format.

use serde::{Deserialize, Serialize};

/// Capability identifier (UCP-style: e.g. dev.ucp.shopping.checkout).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub String);

/// Single capability entry for discovery manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub version: String,
    pub extends: Option<CapabilityId>,
}

/// Discovery manifest (equivalent to /.well-known/ucp): services and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub version: String,
    pub services: std::collections::HashMap<String, ServiceDescriptor>,
    pub capabilities: Vec<CapabilityDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDescriptor {
    pub version: String,
    pub spec_url: Option<String>,
}

impl Default for CapabilityManifest {
    fn default() -> Self {
        Self {
            version: "2026-01-11".to_string(),
            services: std::collections::HashMap::from([(
                "shopping".to_string(),
                ServiceDescriptor {
                    version: "2026-01-11".to_string(),
                    spec_url: Some("https://ucp.dev/specs/shopping".to_string()),
                },
            )]),
            capabilities: vec![
                CapabilityDescriptor {
                    id: CapabilityId("dev.ucp.shopping.checkout".to_string()),
                    version: "2026-01-11".to_string(),
                    extends: None,
                },
                CapabilityDescriptor {
                    id: CapabilityId("dev.ucp.shopping.discount".to_string()),
                    version: "2026-01-11".to_string(),
                    extends: Some(CapabilityId("dev.ucp.shopping.checkout".to_string())),
                },
            ],
        }
    }
}
