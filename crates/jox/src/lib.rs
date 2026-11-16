use std::{
    env,
    sync::LazyLock,
};

mod app;
mod arg;
mod config;
mod fonts;
mod migrate;
mod gstl;
mod macros;

#[cfg(target_os = "linux")]
mod mpris_common;
mod ui;
mod utils;

pub mod client;

pub use arg::Args;
use clap::Parser;
pub use config::*;
use gettextrs::*;
use gtk::prelude::*;

pub use ui::Window;

pub use app::JoxApplication as Application;

use crate::client::runtime::runtime;

pub static USER_AGENT: LazyLock<String> =
    LazyLock::new(|| format!("{}/{} - {}", CLIENT_ID, version(), env::consts::OS));

pub const APP_ID: &str = "com.perwko.jox";
pub const CLIENT_ID: &str = "Jox";
const APP_RESOURCE_PATH: &str = "/com/perwko/jox";
const GRESOURCE_FILE: &str = "jox.gresource";

pub fn run() -> gtk::glib::ExitCode {
    // Before anything reads GSettings, argument handling included.
    migrate::run();
    Args::parse().init();

    // Initialize gettext
    unsafe { setlocale(LocaleCategory::LcAll, String::new()) };
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8").expect("Failed to set textdomain codeset");
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Invalid argument passed to bindtextdomain");

    textdomain(GETTEXT_PACKAGE).expect("Invalid string passed to textdomain");

    fonts::register();
    adw::init().expect("Failed to initialize Adwaita");
    mutsumi::init();

    register_gio_resources();

    ui::init();

    gtk::glib::set_application_name(CLIENT_ID);

    let _tokio_guard = runtime().enter();
    Application::new().run_with_args::<&str>(&[])
}

fn register_gio_resources() {
    let path = std::path::Path::new(PKGDATADIR).join(GRESOURCE_FILE);
    let resources = gtk::gio::Resource::load(path).expect("Failed to load resources.");
    gtk::gio::resources_register(&resources);
}
