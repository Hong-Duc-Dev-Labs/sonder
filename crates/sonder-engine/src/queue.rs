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

    /// Removes the entry at `index`. The current index is left for the caller to remap.
    pub fn remove(&mut self, index: usize) -> Option<PathBuf> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    /// Inserts `paths` before `index` (clamped to the end). The current index is left for the
    /// caller to remap.
    pub fn insert(&mut self, index: usize, paths: impl IntoIterator<Item = PathBuf>) {
        let at = index.min(self.items.len());
        self.items.splice(at..at, paths);
    }

    /// Moves the entry at `from` to `to`. The current index is left for the caller to remap.
    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        true
    }

    /// Reorders entries so `new[i] = old[order[i]]`. Returns `false` if `order` is not a valid
    /// permutation of `0..self.len()`. The current index is left for the caller to remap.
    pub fn reorder(&mut self, order: &[usize]) -> bool {
        if order.len() != self.items.len() {
            return false;
        }
        let mut seen = vec![false; order.len()];
        for &idx in order {
            if idx >= order.len() || seen[idx] {
                return false;
            }
            seen[idx] = true;
        }
        let mut slots: Vec<Option<PathBuf>> = self.items.drain(..).map(Some).collect();
        self.items = order.iter().filter_map(|&idx| slots[idx].take()).collect();
        true
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

    #[test]
    fn move_entry_and_reorder_validate_bounds() {
        let mut q = Queue::default();
        q.extend(["a.flac".into(), "b.flac".into(), "c.flac".into()]);
        assert!(!q.move_entry(1, 1));
        assert!(!q.move_entry(0, 3));
        assert!(q.move_entry(2, 0));
        assert_eq!(
            q.items(),
            &[
                PathBuf::from("c.flac"),
                PathBuf::from("a.flac"),
                PathBuf::from("b.flac")
            ]
        );
        assert!(!q.reorder(&[0, 1]));
        assert!(!q.reorder(&[0, 0, 2]));
        assert!(!q.reorder(&[0, 1, 3]));
        assert!(q.reorder(&[1, 2, 0]));
        assert_eq!(
            q.items(),
            &[
                PathBuf::from("a.flac"),
                PathBuf::from("b.flac"),
                PathBuf::from("c.flac")
            ]
        );
    }
}
