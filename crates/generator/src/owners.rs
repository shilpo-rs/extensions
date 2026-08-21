use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shilpo_ext_api::ExtensionId;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnersConfig {
    #[serde(default)]
    pub namespaces: HashMap<String, Vec<String>>,
}

impl OwnersConfig {
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|err| format!("failed to read owners file at '{}': {err}", path.display()))?;
        toml::from_str(&content)
            .map_err(|err| format!("failed to parse owners file at '{}': {err}", path.display()))
    }

    /// Checks if a given author/login is authorized to publish an extension under the given ID.
    pub fn verify_ownership(
        &self,
        extension_id: &ExtensionId,
        author: Option<&str>,
    ) -> Result<(), String> {
        let id_str = extension_id.as_str();

        // 1. Reserved official namespace
        if id_str.starts_with("org.shilpo.") || id_str == "org.shilpo" {
            let authorized = self
                .namespaces
                .get("org.shilpo")
                .map(|owners| owners.as_slice())
                .unwrap_or(&[]);
            if let Some(author_login) = author {
                if !authorized
                    .iter()
                    .any(|o| o.eq_ignore_ascii_case(author_login))
                {
                    return Err(format!(
                        "namespace 'org.shilpo' is reserved for official maintainers ({:?}); '{}' is not authorized",
                        authorized, author_login
                    ));
                }
            }
            return Ok(());
        }

        // 2. Community namespace check
        // First check exact registered prefix match in owners.toml
        for (namespace, owners) in &self.namespaces {
            let prefix = format!("{namespace}.");
            if id_str.starts_with(&prefix) || id_str == namespace {
                if let Some(author_login) = author {
                    if !owners.iter().any(|o| o.eq_ignore_ascii_case(author_login)) {
                        return Err(format!(
                            "extension ID '{}' matches registered namespace '{namespace}' owned by {:?}, but PR author is '{}'",
                            id_str, owners, author_login
                        ));
                    }
                }
                return Ok(());
            }
        }

        // 3. If not yet in owners.toml (new submission), check community format `io.github.<login>.<extension>`
        if let Some(rest) = id_str.strip_prefix("io.github.") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
                return Err(format!(
                    "community extension ID '{}' must have format 'io.github.<login>.<extension>'",
                    id_str
                ));
            }
            let login = parts[0];
            if let Some(author_login) = author {
                if !login.eq_ignore_ascii_case(author_login) {
                    return Err(format!(
                        "community extension ID '{}' specifies login '{}', but PR author is '{}'",
                        id_str, login, author_login
                    ));
                }
            }
            return Ok(());
        }

        Err(format!(
            "extension ID '{}' does not match any registered namespace in owners.toml or the 'io.github.<login>.*' format",
            id_str
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_official_namespace() {
        let mut config = OwnersConfig::default();
        config
            .namespaces
            .insert("org.shilpo".into(), vec!["sayeed205".into()]);

        let id = ExtensionId::new("org.shilpo.wallpaper").unwrap();
        assert!(config.verify_ownership(&id, Some("sayeed205")).is_ok());
        assert!(config.verify_ownership(&id, Some("attacker")).is_err());
        assert!(config.verify_ownership(&id, None).is_ok());
    }

    #[test]
    fn test_community_namespace() {
        let config = OwnersConfig::default();
        let id = ExtensionId::new("io.github.alice.world-clock").unwrap();
        assert!(config.verify_ownership(&id, Some("alice")).is_ok());
        assert!(config.verify_ownership(&id, Some("bob")).is_err());
    }
}
