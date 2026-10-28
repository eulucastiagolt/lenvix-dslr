//! Lenvix — entry point.
//!
//! Camadas: main → tray (SNI) + core-camera (thread gphoto2) + GUI (GTK4).

use lenvix::{core::camera::CameraEngine, gui::{self, app::activate, UiAction}, tray};


use std::{cell::RefCell, rc::Rc};
use crossbeam_channel::unbounded;
use gtk4::{gio::ApplicationFlags, prelude::*};

fn main() {
    // Intercepta comando administrativo CLI para criação de dispositivo loopback
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "--add-loopback" {
        let nr: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(-1);
        let label = args.get(3).map(|s| s.as_str()).unwrap_or("Lenvix Camera");
        match lenvix::core::stream::add_loopback_root_action(nr, label) {
            Ok(created) => {
                println!("{created}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }

    // core-camera (thread própria; GUI consome eventos)
    let engine = Rc::new(RefCell::new(CameraEngine::spawn()));

    // canais tray → GTK
    let (action_tx, action_rx) = unbounded::<UiAction>();

    // app GTK
    let app = gtk4::Application::new(Some(gui::app::APP_ID), ApplicationFlags::NON_UNIQUE);

    // settings persistidas para estado inicial
    let saved = lenvix::core::settings::load();

    // tray (FR-26/27): roda em thread própria via SNI (KDE/GNOME)
    tray::LenvixTray::spawn(engine.borrow().cmd_tx.clone(), action_tx, saved.webcam);

    app.connect_activate(move |app| {
        activate(app, Rc::clone(&engine), action_rx.clone());
    });

    let _ = app.run();
}
