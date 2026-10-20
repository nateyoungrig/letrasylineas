//! Local users, with no sign-in. Each user has their own SQLite file, so songs, grades,
//! Repaso and the tutor's history never mix between them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Db, Error, Result};

const REGISTRY: &str = "users.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    /// The user's database file, relative to the data directory.
    pub db: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Registry {
    current: String,
    users: Vec<User>,
}

impl Default for Registry {
    /// The songs from before there were users (letras.db) are Nate's.
    fn default() -> Self {
        let user = |id: &str, name: &str, db: &str| User { id: id.into(), name: name.into(), db: db.into() };
        Self {
            current: "nate".into(),
            users: vec![user("nate", "Nate", "letras.db"), user("heather", "Heather", "heather.db")],
        }
    }
}

/// The user list, kept as users.json in the data directory.
pub struct Users {
    dir: PathBuf,
    registry: Registry,
}

impl Users {
    /// Reads the user list from `dir`, writing the default one on first run.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let path = dir.join(REGISTRY);
        let users = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let registry: Registry = serde_json::from_str(&text)
                    .map_err(|e| Error::Invalid(format!("{} is malformed: {e}", path.display())))?;
                if registry.users.is_empty() {
                    return Err(Error::Invalid(format!("{} lists no users", path.display())));
                }
                Self { dir, registry }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let users = Self { dir, registry: Registry::default() };
                users.save()?;
                users
            }
            Err(e) => return Err(e.into()),
        };
        Ok(users)
    }

    pub fn list(&self) -> &[User] {
        &self.registry.users
    }

    /// The user last switched to, or the first one if that user is gone from the list.
    pub fn current(&self) -> &User {
        let users = &self.registry.users;
        users.iter().find(|u| u.id == self.registry.current).unwrap_or(&users[0])
    }

    pub fn open_current(&self) -> Result<Db> {
        Db::open(self.dir.join(&self.current().db))
    }

    /// Makes `id` the current user, remembered across launches, and opens their database.
    pub fn switch(&mut self, id: &str) -> Result<Db> {
        if !self.registry.users.iter().any(|u| u.id == id) {
            return Err(Error::NotFound("user"));
        }
        self.registry.current = id.to_string();
        let db = self.open_current()?;
        self.save()?;
        Ok(db)
    }

    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.registry).expect("registry serializes");
        std::fs::write(self.dir.join(REGISTRY), json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_have_separate_libraries_and_the_choice_persists() {
        let dir = std::env::temp_dir().join(format!("letras-users-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut users = Users::load(&dir).unwrap();
        assert_eq!(users.current().id, "nate");
        assert_eq!(users.current().db, "letras.db", "Nate keeps the existing library");
        let names: Vec<&str> = users.list().iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["Nate", "Heather"]);

        let mut nate = users.open_current().unwrap();
        nate.create_song("Ciudad", "", "Camino solo por la ciudad", "pasted").unwrap();
        let heather = users.switch("heather").unwrap();
        assert!(heather.list_songs().unwrap().is_empty());
        assert!(users.switch("nobody").is_err());

        let users = Users::load(&dir).unwrap();
        assert_eq!(users.current().id, "heather");
        assert_eq!(nate.list_songs().unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
