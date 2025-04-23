use serde::Serialize;

/// A struct representing a bookmark with a name and URL.
#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    /// The name of the bookmark.
    pub name: String,
    /// The URL of the bookmark.
    pub url: String,
}

impl Bookmark {
    /// Creates a new bookmark with the given name and URL.
    pub fn new(name: String, url: String) -> Self {
        Self { name, url }
    }
}
impl std::fmt::Display for Bookmark {
    /// Formats the bookmark as a string.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.url)
    }
}

/// A struct representing a collection of bookmarks.
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
    pub fn remove_bookmark(&mut self, index: usize) -> Result<(), String> {
        if index < self.bookmarks.len() {
            self.bookmarks.remove(index);
            Ok(())
        } else {
            Err(format!("Index {} out of bounds", index))
        }
    }
    /// Gets a reference to a bookmark by its index.
    pub fn get_bookmark(&self, index: usize) -> Option<&Bookmark> {
        if index < self.bookmarks.len() {
            Some(&self.bookmarks[index])
        } else {
            None
        }
    }
    /// Gets all bookmarks.
    pub fn bookmarks(&self) -> &Vec<Bookmark> {
        &self.bookmarks
    }
}
