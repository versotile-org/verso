use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A struct representing a bookmark with a name and URL.
#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    /// The ID of the bookmark.
    pub id: BookmarkId,
    /// The name of the bookmark.
    pub name: String,
    /// The URL of the bookmark.
    pub url: String,
}

impl Bookmark {
    /// Creates a new bookmark with the given name and URL.
    pub fn new(name: String, url: String) -> Self {
        Self {
            id: BookmarkId(url.clone()),
            name,
            url,
        }
    }
}
impl std::fmt::Display for Bookmark {
    /// Formats the bookmark as a string.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.url)
    }
}

/// A struct representing a collection of bookmarks.
/// TODO: Implement a way to save and load bookmarks from a file.
pub struct BookmarkManager {
    /// A vector of bookmarks.
    bookmarks: Vec<Bookmark>,
}
impl BookmarkManager {
    /// Creates a new `BookmarkManager`.
    pub fn new() -> Self {
        Self {
            bookmarks: Vec::new(),
        }
    }

    /// Adds a bookmark to the manager.
    pub fn append_bookmark(&mut self, name: String, url: String) {
        let bookmark = Bookmark::new(name, url);
        self.bookmarks.push(bookmark);
    }

    /// Removes a bookmark from the manager by its index.
    pub fn remove_bookmark(&mut self, id: BookmarkId) -> Result<(), String> {
        if let Some(pos) = self.bookmarks.iter().position(|bookmark| bookmark.id == id) {
            self.bookmarks.remove(pos);
            Ok(())
        } else {
            Err(format!("Bookmark with ID {} not found", id.0))
        }
    }
    
    /// Renames a bookmark
    pub fn rename_bookmark(&mut self, id: BookmarkId, new_name: String) -> Result<(), String> {
        if let Some(bookmark) = self.bookmarks.iter_mut().find(|bookmark| bookmark.id == id) {
            bookmark.name = new_name;
            Ok(())
        } else {
            Err(format!("Bookmark with ID {} not found", id.0))
        }
    }
    /// Gets all bookmarks.
    pub fn bookmarks(&self) -> &Vec<Bookmark> {
        &self.bookmarks
    }
}

/// BookmarkId is a unique identifier for a bookmark.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BookmarkId(String);

impl BookmarkId {
    /// Create a new download id
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl FromStr for BookmarkId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Default for BookmarkId {
    fn default() -> Self {
        Self::new()
    }
}
