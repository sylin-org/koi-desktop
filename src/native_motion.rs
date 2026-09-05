//! GTK preference fallback for the R06 evaluation's WebKit media-query gap.
//! Reads the toolkit's actual value, not a desktop-specific config imitation.
use std::cell::{Cell, RefCell};

use gtk::prelude::*;
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserStyleLevel, UserStyleSheet, WebViewExt,
};

pub fn attach(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|platform| {
        let view = platform.inner();
        let Some(manager) = view.user_content_manager() else {
            eprintln!("Koi cannot attach its native reduced-motion fallback");
            return;
        };
        let Some(settings) = gtk::prelude::WidgetExt::settings(&view) else {
            eprintln!("Koi cannot read its native animation preference");
            return;
        };
        let stylesheet = UserStyleSheet::new(
            koi_ui_spike::REDUCED_MOTION_CSS,
            UserContentInjectedFrames::TopFrame,
            UserStyleLevel::User,
            &[],
            &[],
        );
        let installed = Cell::new(false);
        synchronize(&settings, &manager, &stylesheet, &installed);
        let weak_manager = manager.downgrade();
        let handler = settings.connect_gtk_enable_animations_notify(move |settings| {
            if let Some(manager) = weak_manager.upgrade() {
                synchronize(settings, &manager, &stylesheet, &installed);
            }
        });
        // GtkSettings is session-lived; never leave a per-window subscription on
        // it after destruction. Hiding into the tray keeps the binding alive.
        let handler = RefCell::new(Some(handler));
        view.connect_destroy(move |_| {
            if let Some(handler) = handler.take() {
                settings.disconnect(handler);
            }
        });
    })
}

fn synchronize(
    settings: &gtk::Settings,
    manager: &webkit2gtk::UserContentManager,
    stylesheet: &UserStyleSheet,
    installed: &Cell<bool>,
) {
    let reduce = !settings.is_gtk_enable_animations();
    if installed.replace(reduce) == reduce {
        return;
    }
    if reduce {
        manager.add_style_sheet(stylesheet);
    } else {
        // Preserve every stylesheet owned by Tauri, the document or another
        // integration; only undo this binding's own reduction.
        manager.remove_style_sheet(stylesheet);
    }
}
