//! `brew services list --json`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Service {
    pub name: String,
    /// `started`, `stopped`, `none`, `error`, `scheduled`, … Homebrew adds to
    /// this set over time, so it stays a string rather than a closed enum.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub user: Option<String>,
    /// Path to the launchd plist or systemd unit backing the service.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

impl Service {
    pub fn is_running(&self) -> bool {
        self.status == "started"
    }

    /// A service that stopped on its own with a non-zero code needs attention.
    pub fn has_failed(&self) -> bool {
        self.status == "error" || self.exit_code.is_some_and(|code| code != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICES: &str = include_str!("../../tests/fixtures/services.json");

    #[test]
    fn parses_real_services_output() {
        let services: Vec<Service> = serde_json::from_str(SERVICES).expect("real output parses");
        assert!(!services.is_empty());
        assert!(services.iter().all(|s| !s.name.is_empty()));
    }

    #[test]
    fn classifies_service_state() {
        let running = Service {
            name: "postgresql".into(),
            status: "started".into(),
            user: Some("makar".into()),
            file: None,
            exit_code: None,
        };
        assert!(running.is_running());
        assert!(!running.has_failed());

        let crashed = Service {
            name: "redis".into(),
            status: "stopped".into(),
            user: None,
            file: None,
            exit_code: Some(1),
        };
        assert!(!crashed.is_running());
        assert!(
            crashed.has_failed(),
            "non-zero exit means it needs attention"
        );
    }
}
