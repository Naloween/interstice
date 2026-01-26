use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Into<String> for Version {
    fn into(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}
impl Into<Version> for &str {
    fn into(self) -> Version {
        let parts: Vec<&str> = self.split('.').collect();
        let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        Version {
            major,
            minor,
            patch,
        }
    }
}
