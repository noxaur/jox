//! MPRIS is a D-Bus protocol, so outside Linux these hooks do nothing.

use anyhow::Result;

use crate::ui::{
    mpv::page::MPVPage,
    provider::tu_item::TuItem,
};

impl MPVPage {
    pub async fn initialize_mpris(&self, _app_id: &str) -> Result<()> {
        Ok(())
    }

    pub fn mpris_track_list_changed(&self, _episode_list: &[TuItem]) -> bool {
        false
    }

    pub fn notify_mpris_seeked(&self, _position: i64) {}

    pub fn notify_mpris_track_changed(&self) {}

    pub fn notify_mpris_playing(&self) {}

    pub fn notify_mpris_paused(&self) {}

    pub fn notify_mpris_volume(&self, _volume: f64) {}

    pub fn notify_mpris_stopped(&self) {}

    pub(crate) fn notify_mpris_track_list_replaced(&self) {}
}
