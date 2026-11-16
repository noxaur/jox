use once_cell::sync::Lazy;
use tokio::sync::watch;

pub static VIEWPORT_CHANNEL: Lazy<ViewportChannel> = Lazy::new(|| {
    let (tx, _) = watch::channel(None);
    ViewportChannel { tx }
});

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

impl Viewport {
    pub fn new(width: i32, height: i32, scale: f64) -> Self {
        Self {
            width,
            height,
            scale,
        }
    }
}

pub struct ViewportChannel {
    tx: watch::Sender<Option<Viewport>>,
}

impl ViewportChannel {
    pub fn send(&self, viewport: Viewport) {
        self.tx.send_replace(Some(viewport));
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Viewport>> {
        self.tx.subscribe()
    }
}
