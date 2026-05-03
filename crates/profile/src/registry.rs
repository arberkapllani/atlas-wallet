//! On-disk profile registry.

use crate::error::{ProfileError, ProfileResult};
use crate::types::{Profile, ProfileKind, WatchAccount};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

const REGISTRY_FILENAME: &str = "profiles.json";
const VAULTS_DIRNAME: &str = "vaults";
const WATCH_DIRNAME: &str = "watch";
const LEGACY_VAULT_FILENAME: &str = "vault.bin";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryFile {
    /// Format version (currently `1`). Bump on breaking changes.
    version: u32,
    profiles: Vec<Profile>,
    /// Currently active profile id, if any.
    active: Option<Uuid>,
}

impl RegistryFile {
    fn empty() -> Self {
        Self {
            version: 1,
            profiles: Vec::new(),
            active: None,
        }
    }
}

/// In-memory handle to the on-disk profile registry.
#[derive(Debug)]
pub struct ProfileRegistry {
    data_dir: PathBuf,
    file: RegistryFile,
}

impl ProfileRegistry {
    /// Load the registry from `data_dir/profiles.json`. If absent, create an
    /// empty one (and migrate a legacy `vault.bin` if present).
    pub fn load_or_init(data_dir: impl AsRef<Path>) -> ProfileResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(data_dir.join(VAULTS_DIRNAME))?;
        std::fs::create_dir_all(data_dir.join(WATCH_DIRNAME))?;

        let registry_path = data_dir.join(REGISTRY_FILENAME);
        let file = if registry_path.exists() {
            let bytes = std::fs::read(&registry_path)?;
            serde_json::from_slice::<RegistryFile>(&bytes)?
        } else {
            RegistryFile::empty()
        };

        let mut this = Self { data_dir, file };
        this.migrate_legacy_vault_if_needed()?;
        if !this.file.profiles.is_empty() && this.file.active.is_none() {
            this.file.active = Some(this.file.profiles[0].id);
            this.save()?;
        }
        Ok(this)
    }

    /// If a legacy `vault.bin` exists at the data root and the registry is
    /// empty, move it into a new "Default" hot profile.
    fn migrate_legacy_vault_if_needed(&mut self) -> ProfileResult<()> {
        if !self.file.profiles.is_empty() {
            return Ok(());
        }
        let legacy = self.data_dir.join(LEGACY_VAULT_FILENAME);
        if !legacy.exists() {
            return Ok(());
        }
        let id = Uuid::new_v4();
        let new_relative = PathBuf::from(VAULTS_DIRNAME).join(format!("{id}.bin"));
        let new_abs = self.data_dir.join(&new_relative);
        std::fs::rename(&legacy, &new_abs)?;
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
        self.file.profiles.push(Profile {
            id,
            name: "Default".into(),
            kind: ProfileKind::Hot {
                vault_file: new_relative,
            },
            created_at: now,
        });
        self.file.active = Some(id);
        self.save()?;
        Ok(())
    }

    /// Persist the registry to disk atomically (write to `.tmp`, rename).
    fn save(&self) -> ProfileResult<()> {
        let path = self.data_dir.join(REGISTRY_FILENAME);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(&self.file)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Root data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// All profiles in registration order.
    pub fn profiles(&self) -> &[Profile] {
        &self.file.profiles
    }

    /// The active profile id, if any.
    pub fn active_id(&self) -> Option<Uuid> {
        self.file.active
    }

    /// The active profile, if any.
    pub fn active(&self) -> Option<&Profile> {
        let id = self.file.active?;
        self.file.profiles.iter().find(|p| p.id == id)
    }

    /// Look up a profile by id.
    pub fn get(&self, id: Uuid) -> Option<&Profile> {
        self.file.profiles.iter().find(|p| p.id == id)
    }

    /// Resolve a `vault_file` (relative to `data_dir`) to an absolute path.
    pub fn absolute_path(&self, relative: &Path) -> PathBuf {
        self.data_dir.join(relative)
    }

    /// Set the active profile.
    pub fn set_active(&mut self, id: Uuid) -> ProfileResult<()> {
        if !self.file.profiles.iter().any(|p| p.id == id) {
            return Err(ProfileError::NotFound(id.to_string()));
        }
        self.file.active = Some(id);
        self.save()
    }

    /// Reserve a fresh hot-profile slot. Returns `(profile_id, absolute_vault_path)`.
    /// The caller is responsible for writing the encrypted vault blob to the
    /// returned path. Once the blob is on disk, call [`Self::commit_hot`].
    pub fn reserve_hot(&self, name: &str) -> ProfileResult<(Uuid, PathBuf)> {
        validate_name(name)?;
        if self.file.profiles.iter().any(|p| p.name == name) {
            return Err(ProfileError::NameTaken(name.into()));
        }
        let id = Uuid::new_v4();
        let abs = self.data_dir.join(VAULTS_DIRNAME).join(format!("{id}.bin"));
        Ok((id, abs))
    }

    /// Finalize a hot profile created via [`Self::reserve_hot`]: registers it
    /// in the registry and (by default) marks it active.
    pub fn commit_hot(&mut self, id: Uuid, name: &str) -> ProfileResult<&Profile> {
        if self.file.profiles.iter().any(|p| p.id == id) {
            return Err(ProfileError::Invalid(
                "profile id already registered".into(),
            ));
        }
        let relative = PathBuf::from(VAULTS_DIRNAME).join(format!("{id}.bin"));
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
        let profile = Profile {
            id,
            name: name.to_string(),
            kind: ProfileKind::Hot {
                vault_file: relative,
            },
            created_at: now,
        };
        self.file.profiles.push(profile);
        self.file.active = Some(id);
        self.save()?;
        Ok(self.file.profiles.last().unwrap())
    }

    /// Create a watch-only profile with the given accounts.
    pub fn create_watch_only(
        &mut self,
        name: &str,
        accounts: Vec<WatchAccount>,
    ) -> ProfileResult<&Profile> {
        validate_name(name)?;
        if self.file.profiles.iter().any(|p| p.name == name) {
            return Err(ProfileError::NameTaken(name.into()));
        }
        if accounts.is_empty() {
            return Err(ProfileError::Invalid(
                "watch-only profile needs at least one address".into(),
            ));
        }
        for a in &accounts {
            if a.address.trim().is_empty() {
                return Err(ProfileError::Invalid("empty address".into()));
            }
            if a.chain_id.trim().is_empty() {
                return Err(ProfileError::Invalid("empty chain_id".into()));
            }
        }
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
        let profile = Profile {
            id,
            name: name.to_string(),
            kind: ProfileKind::WatchOnly { accounts },
            created_at: now,
        };
        self.file.profiles.push(profile);
        self.file.active = Some(id);
        self.save()?;
        Ok(self.file.profiles.last().unwrap())
    }

    /// Rename a profile. Fails if the new name is already taken by another.
    pub fn rename(&mut self, id: Uuid, new_name: &str) -> ProfileResult<()> {
        validate_name(new_name)?;
        if self
            .file
            .profiles
            .iter()
            .any(|p| p.id != id && p.name == new_name)
        {
            return Err(ProfileError::NameTaken(new_name.into()));
        }
        let p = self
            .file
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| ProfileError::NotFound(id.to_string()))?;
        p.name = new_name.to_string();
        self.save()
    }

    /// Delete a profile and its on-disk artifacts. Fails if it is the only
    /// remaining profile (the user must always have at least one).
    pub fn delete(&mut self, id: Uuid) -> ProfileResult<()> {
        if self.file.profiles.len() <= 1 {
            return Err(ProfileError::LastProfile);
        }
        let pos = self
            .file
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| ProfileError::NotFound(id.to_string()))?;
        let p = self.file.profiles.remove(pos);
        if let ProfileKind::Hot { vault_file } = &p.kind {
            let abs = self.data_dir.join(vault_file);
            if abs.exists() {
                let _ = std::fs::remove_file(&abs);
            }
        }
        if self.file.active == Some(id) {
            self.file.active = self.file.profiles.first().map(|p| p.id);
        }
        self.save()
    }
}

fn validate_name(name: &str) -> ProfileResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProfileError::Invalid("name cannot be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(ProfileError::Invalid(
            "name must be 64 characters or fewer".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, ProfileRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let reg = ProfileRegistry::load_or_init(dir.path()).unwrap();
        (dir, reg)
    }

    #[test]
    fn empty_registry_starts_with_no_profiles() {
        let (_dir, reg) = fresh();
        assert!(reg.profiles().is_empty());
        assert!(reg.active_id().is_none());
    }

    #[test]
    fn create_and_persist_hot_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = ProfileRegistry::load_or_init(dir.path()).unwrap();
        let (id, vault_path) = reg.reserve_hot("Main").unwrap();
        std::fs::write(&vault_path, b"fake-vault-blob").unwrap();
        let profile = reg.commit_hot(id, "Main").unwrap();
        assert_eq!(profile.name, "Main");
        assert_eq!(profile.id, id);
        assert_eq!(reg.active_id(), Some(id));

        // Round-trip through disk.
        let reg2 = ProfileRegistry::load_or_init(dir.path()).unwrap();
        assert_eq!(reg2.profiles().len(), 1);
        assert_eq!(reg2.active_id(), Some(id));
    }

    #[test]
    fn watch_only_round_trip() {
        let (_dir, mut reg) = fresh();
        let accounts = vec![
            WatchAccount {
                chain_id: "btc".into(),
                address: "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu".into(),
                label: None,
            },
            WatchAccount {
                chain_id: "eth".into(),
                address: "0x9858EfFD232B4033E47d90003D41EC34EcaEda94".into(),
                label: Some("Cold storage".into()),
            },
        ];
        let p = reg.create_watch_only("Cold", accounts.clone()).unwrap();
        assert!(!p.kind.is_signing_capable());
        match &p.kind {
            ProfileKind::WatchOnly { accounts: a } => assert_eq!(a, &accounts),
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn duplicate_name_rejected() {
        let (_dir, mut reg) = fresh();
        let (id, vault_path) = reg.reserve_hot("A").unwrap();
        std::fs::write(&vault_path, b"x").unwrap();
        reg.commit_hot(id, "A").unwrap();
        assert!(matches!(
            reg.reserve_hot("A"),
            Err(ProfileError::NameTaken(_))
        ));
        assert!(matches!(
            reg.create_watch_only(
                "A",
                vec![WatchAccount {
                    chain_id: "btc".into(),
                    address: "bc1q...".into(),
                    label: None,
                }],
            ),
            Err(ProfileError::NameTaken(_))
        ));
    }

    #[test]
    fn cannot_delete_last_profile() {
        let (_dir, mut reg) = fresh();
        let (id, vp) = reg.reserve_hot("Solo").unwrap();
        std::fs::write(&vp, b"x").unwrap();
        reg.commit_hot(id, "Solo").unwrap();
        assert!(matches!(reg.delete(id), Err(ProfileError::LastProfile)));
    }

    #[test]
    fn delete_picks_new_active_and_removes_vault() {
        let (_dir, mut reg) = fresh();
        let (a_id, ap) = reg.reserve_hot("A").unwrap();
        std::fs::write(&ap, b"a").unwrap();
        reg.commit_hot(a_id, "A").unwrap();
        let (b_id, bp) = reg.reserve_hot("B").unwrap();
        std::fs::write(&bp, b"b").unwrap();
        reg.commit_hot(b_id, "B").unwrap();
        reg.set_active(a_id).unwrap();
        reg.delete(a_id).unwrap();
        assert_eq!(reg.active_id(), Some(b_id));
        assert!(!ap.exists());
    }

    #[test]
    fn legacy_vault_bin_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("vault.bin");
        std::fs::write(&legacy, b"legacy-blob").unwrap();
        let reg = ProfileRegistry::load_or_init(dir.path()).unwrap();
        assert_eq!(reg.profiles().len(), 1);
        assert_eq!(reg.profiles()[0].name, "Default");
        assert!(!legacy.exists());
        // Vault is now under vaults/<id>.bin
        match &reg.profiles()[0].kind {
            ProfileKind::Hot { vault_file } => {
                let abs = reg.absolute_path(vault_file);
                assert!(abs.exists());
                assert_eq!(std::fs::read(abs).unwrap(), b"legacy-blob");
            }
            _ => panic!("expected hot"),
        }
    }
}
