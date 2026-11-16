//! Debug builds only. With `JOX_DEV_CONTROL` set to the path of a FIFO, each
//! line written to it drives the window. An agent can then test jox on a
//! display it never focuses, where key and mouse events don't arrive.
//!
//! Lines: `sidebar <index>`, `search <text>`, `item <jellyfin id>`, `back`,
//! `action <name>`.

use std::{
    io::{
        BufRead,
        BufReader,
    },
    path::PathBuf,
};

use gtk::prelude::*;

use super::window::Window;
use crate::{
    client::jellyfin_client::JELLYFIN_CLIENT,
    ui::provider::tu_item::TuItem,
    utils::{
        spawn,
        spawn_tokio,
    },
};

pub fn listen(window: &Window) {
    let Some(path) = std::env::var_os("JOX_DEV_CONTROL").map(PathBuf::from) else {
        return;
    };
    let (sender, receiver) = flume::unbounded::<String>();

    std::thread::spawn(move || {
        loop {
            // Opening a FIFO waits for a writer, and EOF means it left.
            let Ok(file) = std::fs::File::open(&path) else {
                tracing::warn!("Dev control: cannot open {}", path.display());
                return;
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        }
    });

    let window = window.downgrade();
    spawn(async move {
        while let Ok(line) = receiver.recv_async().await {
            let Some(window) = window.upgrade() else {
                return;
            };
            run(&window, line.trim()).await;
        }
    });
}

async fn run(window: &Window, line: &str) {
    tracing::info!("Dev control: {line}");
    let (command, arg) = line.split_once(' ').unwrap_or((line, ""));
    match command {
        "sidebar" => match arg.parse() {
            Ok(index) => window.activate_sidebar(index),
            Err(_) => tracing::warn!("Dev control: `{arg}` is not an index"),
        },
        "search" => {
            if let Some(page) = window.open_search() {
                page.search_for(arg);
            }
        }
        "item" => {
            let id = arg.to_string();
            match spawn_tokio(async move { JELLYFIN_CLIENT.get_item_info(&id).await }).await {
                Ok(item) => TuItem::from_simple(item).activate(window),
                Err(e) => tracing::warn!("Dev control: {e}"),
            }
        }
        "back" => window.on_pop(),
        "action" => {
            if let Err(e) = WidgetExt::activate_action(window, arg, None) {
                tracing::warn!("Dev control: {e}");
            }
        }
        _ => tracing::warn!("Dev control: unknown command `{command}`"),
    }
}
