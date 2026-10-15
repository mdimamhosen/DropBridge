//! Trusted/blocked device list. Holds only public fingerprints, so a plain JSON file
//! is fine; the private identity key lives in OS secure storage (see `Identity`).
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub blocked: bool,
}

pub struct TrustStore {
    path: PathBuf,
    pub devices: BTreeMap<String, Device>,
}

impl TrustStore {
    pub fn load(path: PathBuf) -> Self {
        let devices = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, devices }
    }

    fn save(&self) -> Result<()> {
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.devices)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }

    pub fn is_trusted(&self, fp: &str) -> bool {
        self.devices.get(fp).is_some_and(|d| !d.blocked)
    }

    pub fn is_blocked(&self, fp: &str) -> bool {
        self.devices.get(fp).is_some_and(|d| d.blocked)
    }

    pub fn set(&mut self, fp: &str, name: &str, blocked: bool) -> Result<()> {
        self.devices.insert(fp.to_string(), Device { name: name.to_string(), blocked });
        self.save()
    }

    pub fn forget(&mut self, fp: &str) -> Result<()> {
        self.devices.remove(fp);
        self.save()
    }
}
