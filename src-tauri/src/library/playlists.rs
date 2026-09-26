//! Playlists and listening history.
//!
//! A playlist is an ordered list of library tracks. Positions are rewritten
//! as a dense 0‥n sequence on every change, which keeps reordering a simple
//! "write the new order" operation.

use serde::Serialize;

use super::query::{PLAYLIST_COLUMNS, PlaylistRow, TrackRow, tracks_by_ids};
use super::{LibraryChange, LibraryEvent, LibraryHandle};
use crate::error::{AppError, AppResult};

const MAX_NAME: usize = 200;

fn clean_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidArgument("a playlist needs a name".into()));
    }
    Ok(name.chars().take(MAX_NAME).collect())
}

impl LibraryHandle {
    pub async fn playlists(&self) -> AppResult<Vec<PlaylistRow>> {
        Ok(sqlx::query_as(&format!(
            "SELECT {PLAYLIST_COLUMNS} FROM playlists p ORDER BY p.pinned DESC, p.name COLLATE NOCASE"
        ))
        .fetch_all(self.db().pool())
        .await?)
    }

    pub async fn playlist(&self, id: i64) -> AppResult<PlaylistRow> {
        sqlx::query_as(&format!("SELECT {PLAYLIST_COLUMNS} FROM playlists p WHERE p.id = ?"))
            .bind(id)
            .fetch_optional(self.db().pool())
            .await?
            .ok_or_else(|| AppError::NotFound("playlist".into()))
    }

    /// Creates a playlist, optionally filled with `tracks`.
    pub async fn create_playlist(&self, name: &str, tracks: &[i64]) -> AppResult<i64> {
        let name = clean_name(name)?;
        let (id,): (i64,) = sqlx::query_as("INSERT INTO playlists (name) VALUES (?) RETURNING id")
            .bind(name)
            .fetch_one(self.db().pool())
            .await?;
        if !tracks.is_empty() {
            self.write_order(id, tracks).await?;
        }
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(id)
    }

    pub async fn rename_playlist(&self, id: i64, name: &str) -> AppResult<()> {
        let name = clean_name(name)?;
        let done = sqlx::query("UPDATE playlists SET name = ?, updated_at = unixepoch() WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(self.db().pool())
            .await?;
        if done.rows_affected() == 0 {
            return Err(AppError::NotFound("playlist".into()));
        }
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(())
    }

    pub async fn set_playlist_pinned(&self, id: i64, pinned: bool) -> AppResult<()> {
        sqlx::query("UPDATE playlists SET pinned = ? WHERE id = ?")
            .bind(pinned)
            .bind(id)
            .execute(self.db().pool())
            .await?;
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(())
    }

    pub async fn delete_playlist(&self, id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM playlists WHERE id = ?").bind(id).execute(self.db().pool()).await?;
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(())
    }

    /// The playlist's track ids in order.
    pub async fn playlist_track_ids(&self, id: i64) -> AppResult<Vec<i64>> {
        Ok(sqlx::query_scalar("SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position")
            .bind(id)
            .fetch_all(self.db().pool())
            .await?)
    }

    pub async fn add_to_playlist(&self, id: i64, tracks: &[i64]) -> AppResult<usize> {
        self.playlist(id).await?;
        let mut order = self.playlist_track_ids(id).await?;
        order.extend_from_slice(tracks);
        self.write_order(id, &order).await?;
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(tracks.len())
    }

    /// Removes the entries at `positions` (indexes into the current order).
    pub async fn remove_from_playlist(&self, id: i64, positions: &[usize]) -> AppResult<()> {
        let order: Vec<i64> = self
            .playlist_track_ids(id)
            .await?
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !positions.contains(i))
            .map(|(_, t)| t)
            .collect();
        self.write_order(id, &order).await?;
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(())
    }

    /// Moves the entries at `positions` so they start at index `to` of the
    /// remaining list, keeping their relative order.
    pub async fn move_in_playlist(&self, id: i64, positions: &[usize], to: usize) -> AppResult<()> {
        let current = self.playlist_track_ids(id).await?;
        let order = move_items(&current, positions, to);
        self.write_order(id, &order).await?;
        self.emit(LibraryEvent::Changed(LibraryChange::Playlists));
        Ok(())
    }

    async fn write_order(&self, id: i64, tracks: &[i64]) -> AppResult<()> {
        let mut tx = self.db().pool().begin().await?;
        sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?").bind(id).execute(&mut *tx).await?;
        for (chunk_index, chunk) in tracks.chunks(300).enumerate() {
            let mut q = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO playlist_tracks (playlist_id, position, track_id) ",
            );
            q.push_values(chunk.iter().enumerate(), |mut row, (i, track)| {
                row.push_bind(id).push_bind((chunk_index * 300 + i) as i64).push_bind(*track);
            });
            q.build().execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE playlists SET updated_at = unixepoch() WHERE id = ?").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Most recent plays first, with the track as it is now.
    pub async fn history(&self, limit: i64) -> AppResult<Vec<HistoryEntry>> {
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT track_id, played_at, listened_ms FROM play_history ORDER BY played_at DESC, id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.db().pool())
        .await?;
        let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        let tracks: std::collections::HashMap<i64, TrackRow> =
            tracks_by_ids(self.db(), &unique).await?.into_iter().map(|t| (t.id, t)).collect();
        Ok(rows
            .into_iter()
            .filter_map(|(id, played_at, listened_ms)| {
                Some(HistoryEntry { played_at, listened_ms, track: tracks.get(&id)?.clone() })
            })
            .collect())
    }

    pub async fn clear_history(&self) -> AppResult<()> {
        let mut tx = self.db().pool().begin().await?;
        sqlx::query("DELETE FROM play_history").execute(&mut *tx).await?;
        sqlx::query("UPDATE tracks SET play_count = 0, last_played_at = NULL").execute(&mut *tx).await?;
        tx.commit().await?;
        self.emit(LibraryEvent::Changed(LibraryChange::History));
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub played_at: i64,
    pub listened_ms: i64,
    pub track: TrackRow,
}

/// `items` with the elements at `positions` moved to start at `to`, where
/// `to` indexes the list as it is *after* those elements are taken out.
pub fn move_items<T: Clone>(items: &[T], positions: &[usize], to: usize) -> Vec<T> {
    let mut moving = Vec::new();
    let mut rest = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if positions.contains(&i) {
            moving.push(item.clone());
        } else {
            rest.push(item.clone());
        }
    }
    let to = to.min(rest.len());
    rest.splice(to..to, moving);
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_a_selection_as_a_block() {
        let items = ['a', 'b', 'c', 'd', 'e'];
        assert_eq!(move_items(&items, &[1, 3], 0), ['b', 'd', 'a', 'c', 'e']);
        assert_eq!(move_items(&items, &[0], 99), ['b', 'c', 'd', 'e', 'a']);
        assert_eq!(move_items(&items, &[4], 1), ['a', 'e', 'b', 'c', 'd']);
        assert_eq!(move_items(&items, &[], 2), items);
    }
}
