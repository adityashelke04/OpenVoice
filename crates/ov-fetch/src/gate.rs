use std::path::{Path, PathBuf};

static ACTIVE: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub(crate) struct Claim(PathBuf);
impl Claim {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active.contains(&path) {
            return Err("This model is already downloading.".into());
        }
        active.push(path.clone());
        Ok(Self(path))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        ACTIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|path| path != &self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_destination_is_refused_until_claim_drops() {
        let path = std::env::temp_dir().join("ov-fetch-exclusive");
        let claim = Claim::acquire(&path).unwrap();
        assert!(Claim::acquire(&path).is_err());
        drop(claim);
        assert!(Claim::acquire(&path).is_ok());
    }
}
