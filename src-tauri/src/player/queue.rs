//! The play queue, independent of the library.
//!
//! Entries are kept in insertion order; a separate `order` holds the play
//! order (identical unless shuffle is on) and `cursor` points into it. Turning
//! shuffle on only reshuffles what has not played yet, and turning it off
//! resumes insertion order from the current entry, so history is preserved.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::TrackInfo;

pub type EntryId = u64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub id: EntryId,
    pub track: Arc<TrackInfo>,
}

/// Where newly opened tracks go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InsertMode {
    /// Replace the queue and start playing the first new track.
    Replace,
    /// Add after the current track.
    Next,
    /// Add at the end.
    Append,
}

#[derive(Debug, Default)]
pub struct Queue {
    entries: Vec<QueueEntry>,
    /// Play order as indices into `entries`.
    order: Vec<usize>,
    /// Position of the current entry within `order`.
    cursor: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    next_id: EntryId,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn current(&self) -> Option<&QueueEntry> {
        self.entry_at(self.cursor?)
    }

    /// Position of the current entry in play order.
    pub fn current_index(&self) -> Option<usize> {
        self.cursor
    }

    /// Entries in play order, with the index of the current one.
    pub fn play_order(&self) -> (Vec<QueueEntry>, Option<usize>) {
        (self.order.iter().map(|&i| self.entries[i].clone()).collect(), self.cursor)
    }

    pub fn insert(&mut self, tracks: Vec<Arc<TrackInfo>>, mode: InsertMode) {
        if tracks.is_empty() {
            return;
        }
        if mode == InsertMode::Replace {
            self.entries.clear();
            self.order.clear();
            self.cursor = None;
        }
        let first_new = self.entries.len();
        for track in tracks {
            let id = self.next_id;
            self.next_id += 1;
            self.entries.push(QueueEntry { id, track });
        }
        let mut new: Vec<usize> = (first_new..self.entries.len()).collect();

        match (mode, self.cursor) {
            (InsertMode::Replace, _) => {
                // Keep the first track first so "play these" starts where expected.
                if self.shuffle && new.len() > 1 {
                    fastrand::shuffle(&mut new[1..]);
                }
                self.order = new;
                self.cursor = Some(0);
            }
            (InsertMode::Next, Some(cursor)) => {
                self.order.splice(cursor + 1..cursor + 1, new);
            }
            (_, cursor) => {
                if self.shuffle {
                    fastrand::shuffle(&mut new);
                }
                self.order.extend(new);
                if cursor.is_none() {
                    self.cursor = Some(self.order.len() - (self.entries.len() - first_new));
                }
            }
        }
    }

    /// The entry that should follow the current one when it finishes on its
    /// own. Repeat-one repeats; repeat-all wraps around.
    pub fn peek_auto_next(&self) -> Option<&QueueEntry> {
        let cursor = self.cursor?;
        let next = match self.repeat {
            RepeatMode::One => cursor,
            _ => self.step_forward(cursor)?,
        };
        self.entry_at(next)
    }

    /// Moves to the automatic next entry.
    pub fn advance_auto(&mut self) -> Option<&QueueEntry> {
        let cursor = self.cursor?;
        self.cursor = Some(match self.repeat {
            RepeatMode::One => cursor,
            _ => self.step_forward(cursor)?,
        });
        self.current()
    }

    /// Moves to the next entry because the user asked to. Unlike automatic
    /// advance this skips forward even in repeat-one.
    pub fn advance(&mut self) -> Option<&QueueEntry> {
        self.cursor = Some(self.step_forward(self.cursor?)?);
        self.current()
    }

    pub fn retreat(&mut self) -> Option<&QueueEntry> {
        let cursor = self.cursor?;
        let prev = match cursor.checked_sub(1) {
            Some(p) => p,
            None if self.repeat == RepeatMode::All => self.order.len().checked_sub(1)?,
            None => return None,
        };
        self.cursor = Some(prev);
        self.current()
    }

    pub fn jump_to(&mut self, id: EntryId) -> Option<&QueueEntry> {
        self.cursor = Some(self.position_of(id)?);
        self.current()
    }

    /// Removes an entry. Returns whether it was the current one; if so, the
    /// entry after it (if any) becomes current.
    pub fn remove(&mut self, id: EntryId) -> Option<bool> {
        let index = self.entries.iter().position(|e| e.id == id)?;
        let position = self.order.iter().position(|&i| i == index)?;
        let was_current = self.cursor == Some(position);

        self.entries.remove(index);
        self.order.remove(position);
        for i in &mut self.order {
            if *i > index {
                *i -= 1;
            }
        }
        self.cursor = match self.cursor {
            _ if self.order.is_empty() => None,
            Some(c) if position < c => Some(c - 1),
            Some(c) if was_current && c >= self.order.len() => {
                if self.repeat == RepeatMode::All {
                    Some(0)
                } else {
                    None
                }
            }
            other => other,
        };
        Some(was_current)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.cursor = None;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current = self.cursor.map(|c| self.order[c]);
        if shuffle {
            let start = self.cursor.map_or(0, |c| c + 1);
            fastrand::shuffle(&mut self.order[start..]);
        } else {
            self.order = (0..self.entries.len()).collect();
            self.cursor = current;
        }
    }

    fn step_forward(&self, cursor: usize) -> Option<usize> {
        if cursor + 1 < self.order.len() {
            Some(cursor + 1)
        } else if self.repeat == RepeatMode::All && !self.order.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    fn entry_at(&self, position: usize) -> Option<&QueueEntry> {
        self.order.get(position).map(|&i| &self.entries[i])
    }

    fn position_of(&self, id: EntryId) -> Option<usize> {
        self.order.iter().position(|&i| self.entries[i].id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(names: &[&str]) -> Vec<Arc<TrackInfo>> {
        names.iter().map(|n| Arc::new(TrackInfo::for_test(n))).collect()
    }

    fn titles(q: &Queue) -> Vec<String> {
        q.play_order().0.iter().map(|e| e.track.metadata.title.clone()).collect()
    }

    fn current(q: &Queue) -> Option<String> {
        q.current().map(|e| e.track.metadata.title.clone())
    }

    #[test]
    fn replace_starts_at_first_track_and_advances_in_order() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b", "c"]), InsertMode::Replace);
        assert_eq!(current(&q).as_deref(), Some("a"));
        assert_eq!(q.advance().map(|e| e.track.metadata.title.as_str()), Some("b"));
        assert_eq!(q.advance().map(|e| e.track.metadata.title.as_str()), Some("c"));
        assert!(q.advance().is_none(), "no wrap without repeat");
        assert_eq!(current(&q).as_deref(), Some("c"), "cursor stays on last track");
    }

    #[test]
    fn repeat_modes_control_automatic_advance() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b"]), InsertMode::Replace);
        q.advance();
        assert!(q.peek_auto_next().is_none());

        q.set_repeat(RepeatMode::All);
        assert_eq!(q.peek_auto_next().unwrap().track.metadata.title, "a");

        q.set_repeat(RepeatMode::One);
        assert_eq!(q.peek_auto_next().unwrap().track.metadata.title, "b");
        assert_eq!(q.advance_auto().unwrap().track.metadata.title, "b");
        assert!(q.advance().is_none(), "manual next ignores repeat-one at the end");
    }

    #[test]
    fn insert_next_and_append() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b"]), InsertMode::Replace);
        q.insert(tracks(&["z"]), InsertMode::Append);
        q.insert(tracks(&["n"]), InsertMode::Next);
        assert_eq!(titles(&q), ["a", "n", "b", "z"]);
    }

    #[test]
    fn appending_to_an_empty_queue_selects_the_first_new_track() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b"]), InsertMode::Append);
        assert_eq!(current(&q).as_deref(), Some("a"));
    }

    #[test]
    fn previous_steps_back_and_wraps_only_with_repeat_all() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b"]), InsertMode::Replace);
        assert!(q.retreat().is_none());
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.retreat().unwrap().track.metadata.title, "b");
    }

    #[test]
    fn removing_entries_keeps_the_cursor_on_the_same_track() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b", "c"]), InsertMode::Replace);
        q.advance();
        let first = q.play_order().0[0].id;
        assert_eq!(q.remove(first), Some(false));
        assert_eq!(current(&q).as_deref(), Some("b"));

        let current_id = q.current().unwrap().id;
        assert_eq!(q.remove(current_id), Some(true));
        assert_eq!(current(&q).as_deref(), Some("c"), "next track becomes current");

        let last = q.current().unwrap().id;
        assert_eq!(q.remove(last), Some(true));
        assert!(q.current().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn shuffle_keeps_history_and_current_then_restores_order() {
        let mut q = Queue::default();
        let names: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        q.insert(tracks(&refs), InsertMode::Replace);
        q.advance();
        q.advance();

        q.set_shuffle(true);
        let shuffled = titles(&q);
        assert_eq!(&shuffled[..3], ["t00", "t01", "t02"]);
        assert_eq!(current(&q).as_deref(), Some("t02"));
        let mut sorted = shuffled.clone();
        sorted.sort();
        assert_eq!(sorted, names, "shuffle is a permutation");

        q.set_shuffle(false);
        assert_eq!(titles(&q), names);
        assert_eq!(current(&q).as_deref(), Some("t02"));
    }

    #[test]
    fn jump_to_selects_by_entry_id() {
        let mut q = Queue::default();
        q.insert(tracks(&["a", "b", "c"]), InsertMode::Replace);
        let c = q.play_order().0[2].id;
        assert_eq!(q.jump_to(c).unwrap().track.metadata.title, "c");
        assert!(q.jump_to(999).is_none());
    }
}
