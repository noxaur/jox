use std::cell::RefCell;

use super::tu_item::{
    CardOptions,
    CardShape,
    TuItemBasic,
    TuItemMenuPrelude,
    TuItemOverlay,
    TuItemOverlayPrelude,
    TuItemProgressbarAnimation,
    TuItemProgressbarAnimationPrelude,
};
use crate::ui::provider::tu_item::{
    TuItem,
    item_type::*,
};
use adw::prelude::*;
use glib::Object;
use gtk::{
    gio,
    glib,
    glib::subclass::types::ObjectSubclassIsExt,
    template_callbacks,
};

pub mod imp {
    use std::cell::{
        Cell,
        RefCell,
    };

    use adw::{
        prelude::*,
        subclass::prelude::*,
    };
    use glib::subclass::InitializingObject;
    use gtk::{
        CompositeTemplate,
        PopoverMenu,
        glib,
    };

    use crate::ui::{
        provider::tu_item::TuItem,
        widgets::{
            fixed_bin::FixedBin,
            tu_item::TuItemAction,
        },
    };

    use super::CardOptions;

    // Object holding the state
    #[derive(CompositeTemplate, Default, glib::Properties)]
    #[template(resource = "/com/perwko/jox/ui/listitem.ui")]
    #[properties(wrapper_type = super::TuListItem)]
    pub struct TuListItem {
        #[property(get, set = Self::set_item)]
        pub item: RefCell<TuItem>,
        pub card_options: Cell<CardOptions>,
        pub popover: RefCell<Option<PopoverMenu>>,
        #[template_child]
        pub caption_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub item_frame: TemplateChild<FixedBin>,
        #[template_child]
        pub cover_frame: TemplateChild<FixedBin>,
        #[template_child]
        pub overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub progress_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub direct_play_button: TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TuListItem {
        const NAME: &'static str = "TuListItem";
        type Type = super::TuListItem;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_instance_callbacks();
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for TuListItem {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();

            obj.add_controller(obj.gesture_click());
            obj.set_has_tooltip(true);
            obj.connect_query_tooltip(|obj, _, _, _, tooltip| {
                let name = obj.item().name();
                if name.is_empty() {
                    return false;
                }
                tooltip.set_text(Some(&name));
                true
            });
        }

        fn dispose(&self) {
            if let Some(popover) = self.popover.borrow().as_ref() {
                popover.unparent();
            };
        }
    }

    impl WidgetImpl for TuListItem {}

    impl BinImpl for TuListItem {}

    impl TuListItem {
        pub fn set_item(&self, item: TuItem) {
            let obj = self.obj();
            self.item.replace(item);
            obj.refresh_item();
        }
    }
}

glib::wrapper! {
    pub struct TuListItem(ObjectSubclass<imp::TuListItem>)
        @extends adw::Bin, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl TuItemBasic for TuListItem {
    fn item(&self) -> TuItem {
        self.item()
    }
}

impl TuItemOverlayPrelude for TuListItem {
    fn overlay(&self) -> gtk::Overlay {
        self.imp().overlay.get()
    }

    fn card_options_ext(&self, item: &TuItem) -> CardOptions {
        CardOptions {
            shape: self.effective_card_shape(item),
            ..self.card_options()
        }
    }
}

impl TuItemMenuPrelude for TuListItem {
    fn popover(&self) -> &RefCell<Option<gtk::PopoverMenu>> {
        &self.imp().popover
    }
}

impl TuItemProgressbarAnimationPrelude for TuListItem {
    fn progress_bar(&self) -> gtk::ProgressBar {
        self.imp().progress_bar.get()
    }
}

impl Default for TuListItem {
    fn default() -> Self {
        Self::new(TuItem::default())
    }
}

#[template_callbacks]
impl TuListItem {
    pub fn new(item: TuItem) -> Self {
        Object::builder().property("item", item).build()
    }

    pub fn set_card_options(&self, options: CardOptions) {
        self.imp().card_options.set(options);
    }

    fn card_options(&self) -> CardOptions {
        self.imp().card_options.get()
    }

    fn update_title(&self) {
        let imp = self.imp();
        let item = self.item();

        let title = item.card_title();
        imp.title.set_text(title.as_deref().unwrap_or_default());
        imp.caption_box.set_visible(title.is_some());

        let subtitle = item.card_subtitle().filter(|_| title.is_some());
        imp.subtitle
            .set_text(subtitle.as_deref().unwrap_or_default());
        imp.subtitle.set_visible(subtitle.is_some());
    }

    pub fn refresh_item(&self) {
        let imp = self.imp();
        let item = self.item();

        self.set_picture();

        let (w, h) = self.effective_card_shape(&item).size();

        //  - - - - - - - - - - - - - - -
        // |      |      |       |
        // |      |      |       |
        // | cover frame |       |
        // |      |      |       |
        // |      |      |       |
        //  - - - - - - -    item frame
        // |             |       |
        // |-item frame--|       |
        // |             |       |
        //  - - - - - - - - - - - - - - -
        imp.item_frame.set_fixed_size(w, 0);
        imp.cover_frame.set_fixed_size(w, h);

        if let Some(p) = item.fmt_percentage() {
            self.set_progress(p);
        } else {
            self.clear_progress();
        }

        imp.direct_play_button
            .set_visible(item.has_direct_play_mark());

        self.update_title();
    }

    pub fn unbind_item(&self) {
        let imp = self.imp();

        if let Some(child) = imp.overlay.child() {
            super::picture_loader::PictureLoader::reset_in(&child);
        }
    }

    fn effective_card_shape(&self, item: &TuItem) -> CardShape {
        match self.card_options().shape {
            CardShape::Auto => match item.item_type().as_str() {
                MOVIE | SERIES | BOX_SET | ACTOR | PERSON | DIRECTOR | WRITER | PRODUCER
                | GUEST_STAR | SEASON => CardShape::Portrait,
                VIDEO | MUSIC_VIDEO | ADULT_VIDEO | TV_CHANNEL | COLLECTION_FOLDER | EPISODE
                | USER_VIEW => CardShape::Backdrop,
                _ => CardShape::Square,
            },
            card_shape => card_shape,
        }
    }

    #[template_callback]
    async fn on_play_clicked(&self) {
        let item = self.item();
        if !item.can_direct_play() {
            return;
        }
        item.play_video(self).await;
    }
}
