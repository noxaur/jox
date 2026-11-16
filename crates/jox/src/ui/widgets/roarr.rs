//! roarr in jox: Get on title pages, titles outside the library in search,
//! Downloads and Requests. Everything here is hidden while the active server
//! has no roarr address.

use std::time::Duration;

use adw::prelude::*;
use gettextrs::{
    gettext,
    ngettext,
};
use gtk::{
    gdk,
    gio,
    glib,
    pango,
};

use super::utils::GlobalToast;
use crate::{
    client::{
        error::UserFacingError,
        roarr::{
            ActivityItem,
            AutoRequest,
            DiscoverItem,
            ROARR,
            RequestItem,
            SeriesDetail,
        },
        structs::SimpleListItem,
    },
    utils::{
        spawn,
        spawn_tokio,
    },
};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Runs a roarr request on the tokio runtime and toasts its error, if any.
async fn call<T, F>(widget: &impl IsA<gtk::Widget>, fut: F) -> Option<T>
where
    F: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    match spawn_tokio(fut).await {
        Ok(value) => Some(value),
        Err(e) => {
            widget.toast(e.to_user_facing());
            None
        }
    }
}

async fn get(widget: &impl IsA<gtk::Widget>, request: AutoRequest) {
    if let Some(result) = call(widget, async move { ROARR.auto(&request).await }).await {
        widget.toast(result.summary());
    }
}

/// A poster loaded from a URL, scaled to fill a fixed box.
fn poster(url: Option<String>, width: i32, height: i32) -> gtk::Picture {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(width)
        .height_request(height)
        .can_shrink(true)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["roarr-poster"])
        .build();
    if let Some(url) = url {
        spawn(glib::clone!(
            #[weak]
            picture,
            async move {
                let bytes = spawn_tokio(async move {
                    reqwest::get(url).await?.error_for_status()?.bytes().await
                })
                .await;
                if let Ok(bytes) = bytes
                    && let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
                {
                    picture.set_paintable(Some(&texture));
                }
            }
        ));
    }
    picture
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .single_line_mode(true)
        .css_classes(classes)
        .build()
}

// ---------------------------------------------------------------------------
// Get on title pages

/// Fills `container` with roarr's Get control for a series or movie.
pub fn attach_get(container: &gtk::Box, item: &SimpleListItem) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    container.set_visible(false);
    if !ROARR.is_configured() {
        return;
    }

    let ids = item.provider_ids.clone().unwrap_or_default();
    let tvdb = ids.tvdb.and_then(|id| id.parse::<i64>().ok());
    let tmdb = ids.tmdb.and_then(|id| id.parse::<i64>().ok());

    match item.item_type.as_str() {
        "Series" => {
            let Some(tvdb) = tvdb else {
                return;
            };
            spawn(glib::clone!(
                #[weak]
                container,
                async move {
                    let detail = spawn_tokio(async move { ROARR.series_by_tvdb(tvdb, tmdb).await })
                        .await;
                    if let Ok(detail) = detail {
                        series_get(&container, detail, tmdb.unwrap_or_default());
                    }
                }
            ));
        }
        "Movie" => {
            let Some(tmdb) = tmdb else {
                return;
            };
            spawn(glib::clone!(
                #[weak]
                container,
                async move {
                    let detail = spawn_tokio(async move { ROARR.movie_by_tmdb(tmdb).await }).await;
                    let Ok(detail) = detail else {
                        return;
                    };
                    if detail.queued {
                        container.append(&status(&gettext("Downloading")));
                    } else if !detail.has_file {
                        container.append(&get_button(
                            &gettext("Get Movie"),
                            AutoRequest::movie(tmdb),
                        ));
                    } else {
                        return;
                    }
                    container.set_visible(true);
                }
            ));
        }
        _ => {}
    }
}

fn series_get(container: &gtk::Box, detail: SeriesDetail, tmdb: i64) {
    let missing = detail.missing();
    let queued = detail.queued();

    if missing == 0 {
        if queued > 0 {
            container.append(&status(
                &ngettext(
                    "1 episode downloading",
                    "{} episodes downloading",
                    queued as u32,
                )
                .replace("{}", &queued.to_string()),
            ));
            container.set_visible(true);
        }
        return;
    }

    let label = ngettext("Get 1 Missing", "Get {} Missing", missing as u32)
        .replace("{}", &missing.to_string());
    let request = AutoRequest::series(detail.tvdb_id, tmdb, Vec::new());

    let seasons = detail.missing_by_season();
    if seasons.len() < 2 {
        container.append(&get_button(&label, request));
    } else {
        container.append(&season_split_button(&label, request, &seasons));
    }
    container.set_visible(true);
}

fn status(text: &str) -> gtk::Label {
    label(text, &["dimmed", "roarr-status"])
}

fn get_button(label: &str, request: AutoRequest) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .css_classes(["pill", "roarr-get"])
        .build();
    button.connect_clicked(move |button| {
        let request = request.clone();
        spawn(glib::clone!(
            #[weak]
            button,
            async move {
                button.set_sensitive(false);
                button.set_label(&gettext("Getting…"));
                get(&button, request).await;
                button.set_label(&gettext("Requested"));
            }
        ));
    });
    button
}

/// Get everything missing, with one menu entry per season that has gaps.
fn season_split_button(
    label: &str, request: AutoRequest, seasons: &[(i64, usize)],
) -> adw::SplitButton {
    let button = adw::SplitButton::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .css_classes(["roarr-get"])
        .build();

    let actions = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    for (season, count) in seasons {
        let name = format!("season-{season}");
        let action = gio::SimpleAction::new(&name, None);
        let mut season_request = request.clone();
        season_request.seasons = vec![*season];
        action.connect_activate(glib::clone!(
            #[weak]
            button,
            move |_, _| {
                let request = season_request.clone();
                spawn(async move {
                    button.set_sensitive(false);
                    get(&button, request).await;
                    button.set_sensitive(true);
                });
            }
        ));
        actions.add_action(&action);
        let text = ngettext("Season {s} · 1 missing", "Season {s} · {n} missing", *count as u32)
            .replace("{s}", &season.to_string())
            .replace("{n}", &count.to_string());
        menu.append(Some(&text), Some(&format!("roarr.{name}")));
    }
    button.insert_action_group("roarr", Some(&actions));
    button.set_menu_model(Some(&menu));

    button.connect_clicked(move |button| {
        let request = request.clone();
        spawn(glib::clone!(
            #[weak]
            button,
            async move {
                button.set_sensitive(false);
                button.set_label(&gettext("Getting…"));
                get(&button, request).await;
                button.set_label(&gettext("Requested"));
            }
        ));
    });
    button
}

// ---------------------------------------------------------------------------
// Search: titles that are not in the library yet

/// A strip on the search page with roarr's matches outside the library.
pub struct DiscoverSection {
    pub root: gtk::Box,
    strip: gtk::Box,
    /// Bumped by every search, so a slow answer to an old query is dropped.
    generation: std::rc::Rc<std::cell::Cell<u64>>,
}

impl Default for DiscoverSection {
    fn default() -> Self {
        let strip = gtk::Box::builder()
            .spacing(16)
            .margin_bottom(6)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&strip)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_start(15)
            .margin_end(15)
            .visible(false)
            .build();
        root.append(&label(&gettext("Not in Your Library"), &["title-4"]));
        root.append(&scroller);
        Self {
            root,
            strip,
            generation: Default::default(),
        }
    }
}

impl DiscoverSection {
    pub fn search(&self, query: &str) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        while let Some(child) = self.strip.first_child() {
            self.strip.remove(&child);
        }
        self.root.set_visible(false);
        if !ROARR.is_configured() || query.trim().is_empty() {
            return;
        }

        let query = query.to_owned();
        let root = self.root.clone();
        let strip = self.strip.clone();
        let latest = self.generation.clone();
        spawn(async move {
            let results = spawn_tokio(async move { ROARR.discover(&query).await }).await;
            if latest.get() != generation {
                return;
            }
            let Ok(results) = results else {
                return;
            };
            let outside: Vec<_> = results.into_iter().filter(|r| !r.in_library).collect();
            for item in outside.iter().take(18) {
                strip.append(&discover_card(item));
            }
            root.set_visible(!outside.is_empty());
        });
    }
}

fn discover_card(item: &DiscoverItem) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .width_request(150)
        .css_classes(["jox-card"])
        .build();
    card.append(&poster(ROARR.image_url(&item.poster), 150, 225));

    let kind = if item.kind == "movie" {
        gettext("Movie")
    } else {
        gettext("Show")
    };
    let subtitle = if item.year > 0 {
        format!("{kind} · {}", item.year)
    } else {
        kind
    };
    card.append(&label(&item.title, &["jox-card-title"]));
    card.append(&label(&subtitle, &["jox-card-subtitle"]));

    let request = if item.kind == "movie" {
        AutoRequest::movie(item.tmdb_id)
    } else {
        AutoRequest::series(item.tvdb_id, item.tmdb_id, Vec::new())
    };
    let button = get_button(&gettext("Get"), request);
    button.set_halign(gtk::Align::Start);
    button.add_css_class("small");
    card.append(&button);
    card
}

// ---------------------------------------------------------------------------
// Downloads

pub fn downloads_page() -> gtk::Widget {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let empty = adw::StatusPage::builder()
        .icon_name("folder-download-symbolic")
        .title(gettext("Nothing Is Downloading"))
        .description(gettext("Titles you get through roarr show up here until they land in Jellyfin."))
        .css_classes(["compact"])
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&page_column(&list), Some("list"));
    stack.add_named(&empty, Some("empty"));
    stack.set_visible_child_name("empty");

    poll(&stack, move |stack| {
        let list = list.clone();
        let stack = stack.clone();
        async move {
            let Some(items) = call(&stack, async { ROARR.activity().await }).await else {
                return;
            };
            list.remove_all();
            for item in &items {
                list.append(&activity_row(item));
            }
            stack.set_visible_child_name(if items.is_empty() { "empty" } else { "list" });
        }
    });
    stack.upcast()
}

fn activity_row(item: &ActivityItem) -> gtk::ListBoxRow {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text.append(&label(&item.title, &["jox-row-title"]));

    let mut details = vec![];
    if !item.subtitle.is_empty() {
        details.push(item.subtitle.clone());
    }
    details.push(activity_status(&item.status));
    if item.size > 0 {
        details.push(format!(
            "{} / {}",
            glib::format_size((item.size - item.size_left).max(0) as u64),
            glib::format_size(item.size as u64)
        ));
    }
    if item.speed > 0 {
        details.push(format!("{}/s", glib::format_size(item.speed as u64)));
    }
    if !item.eta.is_empty() {
        details.push(item.eta.clone());
    }
    let classes: &[&str] = if matches!(item.status.as_str(), "warning" | "failed" | "stalled") {
        &["jox-row-subtitle", "warning"]
    } else {
        &["jox-row-subtitle"]
    };
    text.append(&label(&details.join(" · "), classes));
    if let Some(message) = item.messages.first() {
        let message = label(message, &["jox-row-subtitle", "dimmed"]);
        message.set_tooltip_text(Some(&item.messages.join("\n")));
        text.append(&message);
    }

    let progress = gtk::ProgressBar::builder()
        .fraction(item.progress.clamp(0.0, 1.0))
        .css_classes(["jox-progress"])
        .margin_top(4)
        .build();
    text.append(&progress);

    let row = gtk::Box::builder().spacing(12).margin_top(10).margin_bottom(10).margin_start(12).margin_end(12).build();
    row.append(&poster(ROARR.image_url(&item.poster), 40, 60));
    row.append(&text);

    gtk::ListBoxRow::builder()
        .activatable(false)
        .child(&row)
        .build()
}

fn activity_status(status: &str) -> String {
    match status {
        "downloading" => gettext("Downloading"),
        "queued" => gettext("Queued"),
        "paused" => gettext("Paused"),
        "importing" => gettext("Importing"),
        "warning" => gettext("Needs attention"),
        "failed" => gettext("Failed"),
        "stalled" => gettext("Stalled"),
        other => other.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Requests

pub fn requests_page() -> gtk::Widget {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let empty = adw::StatusPage::builder()
        .icon_name("mail-inbox-symbolic")
        .title(gettext("No Requests"))
        .css_classes(["compact"])
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&page_column(&list), Some("list"));
    stack.add_named(&empty, Some("empty"));
    stack.set_visible_child_name("empty");

    poll(&stack, move |stack| {
        let list = list.clone();
        let stack = stack.clone();
        let empty = empty.clone();
        async move {
            let Some(requests) = call(&stack, async { ROARR.requests().await }).await else {
                return;
            };
            if !requests.configured {
                empty.set_description(Some(&gettext(
                    "Requests come from Seerr. Connect it in roarr to see them here.",
                )));
            } else {
                empty.set_description(None);
            }
            list.remove_all();
            for item in &requests.items {
                list.append(&request_row(item, &stack));
            }
            stack.set_visible_child_name(if requests.items.is_empty() {
                "empty"
            } else {
                "list"
            });
        }
    });
    stack.upcast()
}

fn request_row(item: &RequestItem, page: &gtk::Stack) -> gtk::ListBoxRow {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title = if item.year > 0 {
        format!("{} ({})", item.title, item.year)
    } else {
        item.title.clone()
    };
    text.append(&label(&title, &["jox-row-title"]));
    let mut details = vec![];
    if !item.requested_by.is_empty() {
        details.push(gettext("Requested by {}").replace("{}", &item.requested_by));
    }
    if !item.seasons.is_empty() {
        let seasons: Vec<_> = item.seasons.iter().map(|s| s.to_string()).collect();
        details.push(gettext("Seasons {}").replace("{}", &seasons.join(", ")));
    }
    details.push(request_status(item));
    text.append(&label(&details.join(" · "), &["jox-row-subtitle"]));

    let row = gtk::Box::builder().spacing(12).margin_top(10).margin_bottom(10).margin_start(12).margin_end(12).build();
    row.append(&poster(ROARR.image_url(&item.poster), 40, 60));
    row.append(&text);

    if item.status == "pending" {
        let actions = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
        let decline = gtk::Button::builder()
            .label(gettext("Decline"))
            .css_classes(["flat", "pill", "small"])
            .build();
        let approve = gtk::Button::builder()
            .label(gettext("Approve"))
            .css_classes(["pill", "small"])
            .build();
        let approve_get = gtk::Button::builder()
            .label(gettext("Approve & Get"))
            .css_classes(["suggested-action", "pill", "small"])
            .build();

        let id = item.id;
        let get_request = if item.kind == "movie" {
            AutoRequest::movie(item.tmdb_id)
        } else {
            AutoRequest::series(item.tvdb_id, item.tmdb_id, item.seasons.clone())
        };
        let act = move |page: gtk::Stack, buttons: gtk::Box, action: &'static str, then_get: bool| {
            let get_request = get_request.clone();
            spawn(async move {
                buttons.set_sensitive(false);
                if call(&page, async move { ROARR.request_action(id, action).await })
                    .await
                    .is_some()
                    && then_get
                {
                    get(&page, get_request).await;
                }
                refresh(&page);
            });
        };
        let act_decline = act.clone();
        let act_approve = act.clone();
        decline.connect_clicked(glib::clone!(
            #[weak]
            page,
            #[weak]
            actions,
            move |_| act_decline(page, actions, "decline", false)
        ));
        approve.connect_clicked(glib::clone!(
            #[weak]
            page,
            #[weak]
            actions,
            move |_| act_approve(page, actions, "approve", false)
        ));
        approve_get.connect_clicked(glib::clone!(
            #[weak]
            page,
            #[weak]
            actions,
            move |_| act(page, actions, "approve", true)
        ));
        actions.append(&decline);
        actions.append(&approve);
        actions.append(&approve_get);
        row.append(&actions);
    }

    gtk::ListBoxRow::builder()
        .activatable(false)
        .child(&row)
        .build()
}

fn request_status(item: &RequestItem) -> String {
    match (item.status.as_str(), item.available.as_str()) {
        (_, "available") => gettext("Available"),
        (_, "partial") => gettext("Partly available"),
        (_, "processing") => gettext("Downloading"),
        ("pending", _) => gettext("Waiting for approval"),
        ("approved", _) => gettext("Approved"),
        ("declined", _) => gettext("Declined"),
        ("failed", _) => gettext("Failed"),
        ("completed", _) => gettext("Done"),
        (other, _) => other.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Page plumbing

fn page_column(list: &gtk::ListBox) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder()
        .maximum_size(860)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(16)
        .margin_end(16)
        .child(list)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build()
}

type Refresh = std::rc::Rc<dyn Fn(&gtk::Stack) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>>;

/// Refreshes `page` now and then every few seconds while it is on screen.
fn poll<F, Fut>(page: &gtk::Stack, refresh: F)
where
    F: Fn(&gtk::Stack) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    let refresh: Refresh = std::rc::Rc::new(move |page| Box::pin(refresh(page)));
    unsafe { page.set_data("roarr-refresh", refresh.clone()) };

    let tick = glib::clone!(
        #[weak]
        page,
        #[upgrade_or]
        glib::ControlFlow::Break,
        move || {
            if page.is_mapped() {
                spawn(refresh(&page));
            }
            glib::ControlFlow::Continue
        }
    );
    page.connect_map(|page| self::refresh(page));
    glib::timeout_add_local(POLL_INTERVAL, tick);
}

/// Refreshes a Downloads or Requests page right away.
pub fn refresh(page: &gtk::Stack) {
    let refresh = unsafe { page.data::<Refresh>("roarr-refresh") };
    if let Some(refresh) = refresh {
        let refresh = unsafe { refresh.as_ref() }.clone();
        spawn(refresh(page));
    }
}
