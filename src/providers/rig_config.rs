use serde::{Deserialize, Serialize};

/// The pre-built Rig provider vendor types supported by the config surface.
///
/// Each vendor drives the shape of the captured config form (which fields are
/// shown) and, in RIG 1, the Rig `ClientBuilder` used to execute against it.
/// This module only captures configuration — no execution happens here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RigVendor {
    /// Anthropic service (api key only; no base_url).
    Anthropic,
    /// OpenAI service (api key only; no base_url).
    OpenAi,
    /// OpenCode Go (api key; fixed OpenAI-compatible-style base URL).
    OpenCodeGo,
    /// OpenRouter (api key; fixed bindable base URL).
    OpenRouter,
    /// OpenAI-compatible endpoint with an arbitrary base_url and a Bearer
    /// api-key auth header.
    OpenAiCompatible,
    /// OpenAI-compatible endpoint with an arbitrary base_url and **no** auth
    /// header.
    OpenAiCompatibleNoAuth,
}

impl RigVendor {
    pub const ALL: [RigVendor; 6] = [
        RigVendor::Anthropic,
        RigVendor::OpenAi,
        RigVendor::OpenCodeGo,
        RigVendor::OpenRouter,
        RigVendor::OpenAiCompatible,
        RigVendor::OpenAiCompatibleNoAuth,
    ];

    /// Human-readable label for UI dropdowns / sub-view headings.
    pub fn label(self) -> &'static str {
        match self {
            RigVendor::Anthropic => "Anthropic",
            RigVendor::OpenAi => "OpenAI (service)",
            RigVendor::OpenCodeGo => "OpenCode Go",
            RigVendor::OpenRouter => "OpenRouter",
            RigVendor::OpenAiCompatible => "OpenAI-compatible (base_url + Bearer)",
            RigVendor::OpenAiCompatibleNoAuth => "OpenAI-compatible (base_url, no auth)",
        }
    }

    /// Whether the capture form should show a base_url input for this vendor.
    ///
    /// Service providers (Anthropic / OpenAI) use their built-in endpoint;
    /// OpenCode Go and OpenRouter have a fixed default URL but still allow an
    /// override; the OpenAI-compatible variants always require one.
    pub fn accepts_base_url(self) -> bool {
        !matches!(self, RigVendor::Anthropic | RigVendor::OpenAi)
    }

    /// Whether the vendor's base_url is mandatory (no sensible default).
    pub fn requires_base_url(self) -> bool {
        matches!(
            self,
            RigVendor::OpenAiCompatible | RigVendor::OpenAiCompatibleNoAuth
        )
    }

    /// Fixed default base URL, where one exists.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            RigVendor::OpenCodeGo => Some("https://opencode.ai/zen/go/v1"),
            _ => None,
        }
    }

    /// Whether a provider config of this vendor must carry an api key.
    pub fn requires_api_key(self) -> bool {
        !matches!(self, RigVendor::OpenAiCompatibleNoAuth)
    }
}

/// How a provider's available models are captured.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelListMode {
    /// Provider exposes an OpenAPI-compatible model-listing API
    /// (`GET {base_url}/v1/models`); consumed at execution time (RIG 1).
    #[default]
    OpenApiList,
    /// User supplies model ids manually (single model or explicit list).
    Manual(Vec<String>),
}

impl ModelListMode {
    /// The model ids exposed by this mode: the manual list if Manual, else the
    /// models cached on the config (which may be empty until a listing fetch
    /// lands in RIG 1).
    pub fn models(&self, cached: &[String]) -> Vec<String> {
        match self {
            ModelListMode::OpenApiList => cached.to_vec(),
            ModelListMode::Manual(models) => models.clone(),
        }
    }
}

/// A captured Rig-based provider configuration, persisted as a structured JSON
/// file under `{config_root}/provider-configs/{uuid}.rig.json` and referenced
/// by the `user_model_configs` row (`harness = "rig"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RigProviderConfig {
    pub name: String,
    pub vendor: RigVendor,
    /// None => vendor default endpoint.
    pub base_url: Option<String>,
    /// None => no-auth (only valid for the no-auth variant).
    pub api_key: Option<String>,
    pub model_list_mode: ModelListMode,
    /// Cached/last-fetched or manual model id list.
    #[serde(default)]
    pub models: Vec<String>,
}

impl RigProviderConfig {
    /// Structural validation of a captured config. Returns a user-facing
    /// message on the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("provider name is required".into());
        }
        if self.vendor.requires_base_url() && is_blank(self.base_url.as_deref()) {
            return Err(format!(
                "base_url is required for vendor '{}'",
                self.vendor.label()
            ));
        }
        if self.vendor.accepts_base_url() {
            if let Some(base_url) = self.base_url.as_deref() {
                if !is_safe_base_url(base_url) {
                    return Err(
                        "base_url must be an http(s) URL to a public endpoint (internal hosts are not allowed)".into(),
                    );
                }
            }
        }
        if self.vendor.requires_api_key() && is_blank(self.api_key.as_deref()) {
            return Err(format!(
                "api_key is required for vendor '{}'",
                self.vendor.label()
            ));
        }
        if let ModelListMode::Manual(models) = &self.model_list_mode {
            if models.iter().all(|m| m.trim().is_empty()) {
                return Err("at least one model is required when model listing is 'manual'".into());
            }
        }
        Ok(())
    }

    /// The model ids exposed by this config (manual list or cached list).
    pub fn available_models(&self) -> Vec<String> {
        self.model_list_mode.models(&self.models)
    }
}

fn is_blank(value: Option<&str>) -> bool {
    value.map(str::trim).unwrap_or_default().is_empty()
}

/// Whether `base_url` is an acceptable provider endpoint: an `http(s)` URL
/// pointing at a non-internal host. Capture-time hardening against a latent
/// SSRF primitive — RIG 1 must re-validate at execution time before making
/// any request to the captured URL.
fn is_safe_base_url(base_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(base_url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    match parsed.host() {
        Some(url::Host::Ipv4(ip)) => !(ip.is_loopback() || ip.is_private() || ip.is_link_local()),
        Some(url::Host::Ipv6(ip)) => {
            !(ip.is_loopback() || ip.is_unicast_link_local() || ip.is_unique_local())
        }
        Some(url::Host::Domain(host)) => {
            let host = host.to_ascii_lowercase();
            host != "localhost"
                && !host.ends_with(".localhost")
                && !host.ends_with(".local")
                && !host.ends_with(".internal")
                && host != "metadata.google.internal"
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cfg(vendor: RigVendor) -> RigProviderConfig {
        RigProviderConfig {
            name: "my-provider".into(),
            vendor,
            base_url: Some("https://example.com/v1".into()),
            api_key: Some("sk-123".into()),
            model_list_mode: ModelListMode::Manual(vec!["gpt-4".into(), "gpt-4o".into()]),
            models: vec!["cached-1".into()],
        }
    }

    #[test]
    fn test_rig_vendor_serde_snake_case() {
        let cases = [
            (RigVendor::Anthropic, "anthropic"),
            (RigVendor::OpenAi, "open_ai"),
            (RigVendor::OpenCodeGo, "open_code_go"),
            (RigVendor::OpenRouter, "open_router"),
            (RigVendor::OpenAiCompatible, "open_ai_compatible"),
            (
                RigVendor::OpenAiCompatibleNoAuth,
                "open_ai_compatible_no_auth",
            ),
        ];
        for (vendor, tag) in cases {
            assert_eq!(
                serde_json::to_string(&vendor).unwrap(),
                format!("\"{tag}\"")
            );
            let back: RigVendor = serde_json::from_str(&format!("\"{tag}\"")).unwrap();
            assert_eq!(back, vendor);
        }
    }

    #[test]
    fn test_model_list_mode_round_trip() {
        let mode = ModelListMode::Manual(vec!["a".into(), "b".into()]);
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("manual"));
        let back: ModelListMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);

        let mode = ModelListMode::OpenApiList;
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("open_api_list"));
        let back: ModelListMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }

    #[test]
    fn test_rig_provider_config_round_trip() {
        for vendor in RigVendor::ALL {
            let cfg = sample_cfg(vendor);
            let json = serde_json::to_string_pretty(&cfg).unwrap();
            let back: RigProviderConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cfg, "round-trip failed for {vendor:?}");
        }
    }

    #[test]
    fn test_available_models_manual() {
        let cfg = sample_cfg(RigVendor::OpenAi);
        assert_eq!(cfg.available_models(), vec!["gpt-4", "gpt-4o"]);
    }

    #[test]
    fn test_available_models_open_api_list() {
        let mut cfg = sample_cfg(RigVendor::OpenAi);
        cfg.model_list_mode = ModelListMode::OpenApiList;
        assert_eq!(cfg.available_models(), vec!["cached-1"]);
    }

    #[test]
    fn test_validate_ok() {
        assert!(sample_cfg(RigVendor::Anthropic).validate().is_ok());
        assert!(sample_cfg(RigVendor::OpenAiCompatibleNoAuth)
            .validate()
            .is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let mut cfg = sample_cfg(RigVendor::OpenAi);
        cfg.name = "  ".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_missing_base_url() {
        let mut cfg = sample_cfg(RigVendor::OpenAiCompatible);
        cfg.base_url = None;
        assert!(cfg.validate().is_err());
        // Anthropic does not need a base_url
        let mut cfg2 = sample_cfg(RigVendor::Anthropic);
        cfg2.base_url = None;
        assert!(cfg2.validate().is_ok());
    }

    #[test]
    fn test_validate_missing_api_key() {
        let mut cfg = sample_cfg(RigVendor::OpenAi);
        cfg.api_key = None;
        assert!(cfg.validate().is_err());
        // NoAuth variant does not need an api key
        let mut cfg2 = sample_cfg(RigVendor::OpenAiCompatibleNoAuth);
        cfg2.api_key = None;
        assert!(cfg2.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_internal_base_url() {
        for bad in [
            "http://127.0.0.1:11434/v1",
            "http://[::1]:8080/v1",
            "http://localhost:8080/v1",
            "http://192.168.1.10/v1",
            "http://10.0.0.5/v1",
            "http://169.254.169.254/latest/meta-data",
            "http://foo.internal/v1",
            "http://myhost.local/v1",
            "https://metadata.google.internal/computeMetadata",
            "ftp://example.com/v1",
            "file:///etc/passwd",
            "not-a-url",
        ] {
            let mut cfg = sample_cfg(RigVendor::OpenAiCompatible);
            cfg.base_url = Some(bad.into());
            assert!(cfg.validate().is_err(), "expected rejection for {bad}");
        }
    }

    #[test]
    fn test_validate_accepts_public_base_url() {
        for good in [
            "https://api.openai.com/v1",
            "https://opencode.ai/zen/go/v1",
            "http://example.com/v1",
            "https://api.anthropic.com",
        ] {
            let mut cfg = sample_cfg(RigVendor::OpenAiCompatible);
            cfg.base_url = Some(good.into());
            assert!(cfg.validate().is_ok(), "expected acceptance for {good}");
        }
    }

    #[test]
    fn test_validate_manual_empty_models() {
        let mut cfg = sample_cfg(RigVendor::OpenAi);
        cfg.model_list_mode = ModelListMode::Manual(vec![]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_open_code_go_default_base_url() {
        assert_eq!(
            RigVendor::OpenCodeGo.default_base_url(),
            Some("https://opencode.ai/zen/go/v1")
        );
        assert!(RigVendor::Anthropic.default_base_url().is_none());
    }
}
