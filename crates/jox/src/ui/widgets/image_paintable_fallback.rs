//! glycin decodes in sandboxed loader processes that only exist on Linux, so
//! elsewhere images go through GTK's own loaders. Animations show their first
//! frame only.

use anyhow::{
    Result,
    bail,
};
use gtk::{
    gdk,
    gio,
    prelude::*,
};

pub async fn paintable_from_file(
    file: gio::File, cancellable: Option<gio::Cancellable>,
) -> Result<gdk::Paintable> {
    if cancellable.as_ref().is_some_and(|c| c.is_cancelled()) {
        bail!("image load cancelled");
    }

    let (bytes, _) = file.load_bytes_future().await?;
    let texture = gio::spawn_blocking(move || gdk::Texture::from_bytes(&bytes))
        .await
        .map_err(|_| anyhow::anyhow!("image decoder panicked"))??;

    Ok(texture.upcast())
}
