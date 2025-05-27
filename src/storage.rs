use directories::ProjectDirs;
use std::{fs::create_dir_all, path::PathBuf};

use crate::bookmark::BookmarkStorage;

#[derive(Default)]
pub(crate) struct Storage {
    bookmark_storage: Option<BookmarkStorage>,
}

impl Storage {
    pub fn new() -> Self {
        let project_dir = ProjectDirs::from("org", "versotile", "verso");

        let config_dir_path = Self::get_and_create_config_dir_path(project_dir);
        if config_dir_path.is_none() {
            return Self::default();
        }

        let bookmark_storage = BookmarkStorage::new(config_dir_path.unwrap());

        Self {
            bookmark_storage: Some(bookmark_storage),
        }
    }

    fn get_and_create_config_dir_path(project_dir: Option<ProjectDirs>) -> Option<PathBuf> {
        if project_dir.is_none() {
            log::error!("Project directory not found");
            return None;
        }

        let config_path = project_dir.unwrap().config_dir().to_path_buf();

        if create_dir_all(&config_path).is_err() {
            log::error!(
                "Failed to create config directory: {}",
                config_path.display()
            );
            return None;
        }

        Some(config_path)
    }

    pub(crate) fn bookmark_storage(&self) -> Option<&BookmarkStorage> {
        self.bookmark_storage.as_ref()
    }
}
