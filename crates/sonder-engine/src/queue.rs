//! The play queue: an ordered list of files and the index the user considers current.

use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Queue {
    items: Vec<PathBuf>,
    current: Option<usize>,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Path> {
        self.items.get(index).map(PathBuf::as_path)
    }

    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    /// The track playing, paused, or selected to play next from a stop.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn set_current(&mut self, index: Option<usize>) {
        self.current = index.filter(|&i| i < self.items.len());
    }

    pub fn extend(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.items.extend(paths);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current = None;
    }

    /// Where playback starts from a stop: the current track, else the first one.
    pub fn start_index(&self) -> Option<usize> {
        self.current
            .or(if self.items.is_empty() { None } else { Some(0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_never_points_past_the_end() {
        let mut q = Queue::default();
        assert_eq!(q.start_index(), None);
        q.extend(["a.flac".into(), "b.flac".into()]);
        assert_eq!(q.start_index(), Some(0));
        q.set_current(Some(5));
        assert_eq!(q.current(), None);
        q.set_current(Some(1));
        assert_eq!(q.start_index(), Some(1));
        q.clear();
        assert_eq!(q.current(), None);
        assert!(q.is_empty());
    }
}
