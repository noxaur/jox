//! MPRIS is a D-Bus protocol, so outside Linux these hooks do nothing.

use anyhow::Result;

use super::player::MusicPlayer;

impl MusicPlayer {
    pub async fn initialize_mpris(&self) -> Result<()> {
        Ok(())
    }

    pub fn notify_mpris_seeked(&self, _position: i64) {}

    pub fn notify_mpris_song_changed(&self, _has_prev: bool, _has_next: bool) {}

    pub fn notify_mpris_playing(&self) {}

    pub fn notify_mpris_paused(&self) {}

    pub fn notify_mpris_stopped(&self) {}
}
