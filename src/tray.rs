//! Tray: StatusNotifierItem via ksni (bloqueante/threads próprios).
//! Menu fiel FR-27: abrir/fechar janela, webcam on/off, sair (com limpeza).

use crate::core::camera::EngineCmd;
use crate::gui::UiAction;
use crossbeam_channel::Sender;
use ksni::{
    menu::{CheckmarkItem, StandardItem},
    Icon, MenuItem, Tray,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct LenvixTray {
    pub cmd_tx: Sender<EngineCmd>,
    pub action_tx: Sender<UiAction>,
    webcam_on: Arc<AtomicBool>,
}

impl LenvixTray {
    pub fn spawn(cmd_tx: Sender<EngineCmd>, action_tx: Sender<UiAction>, initial_webcam_on: bool) {
        let tray = Self {
            cmd_tx,
            action_tx,
            webcam_on: Arc::new(AtomicBool::new(initial_webcam_on)),
        };
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || {
                if let Err(e) = tray.spawn_blocking() {
                    // sem watchers (DE sem SNI): sem tray, app continua gracioso
                    eprintln!("tray indisponível (SNI): {e}");
                }
            })
            .expect("spawn tray");
    }
}

impl LenvixTray {
    fn spawn_blocking(self) -> Result<ksni::blocking::Handle<Self>, ksni::Error> {
        ksni::blocking::TrayMethods::spawn(self)
    }
}

impl Tray for LenvixTray {
    fn id(&self) -> String {
        "lenvix".into()
    }

    fn icon_name(&self) -> String {
        // ícone padrão de câmera (fallback do tema; ícone próprio fica para depois)
        "camera-photo-symbolic".into()
    }

    fn title(&self) -> String {
        "Lenvix".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: vec![Icon { width: 0, height: 0, data: Vec::new() }],
            title: "Lenvix".into(),
            description: "Câmera DSLR → webcam virtual".into(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let webcam_on = self.webcam_on.load(Ordering::Relaxed);
        let web_state = Arc::clone(&self.webcam_on);
        let action_show = self.action_tx.clone();
        let action_quit = self.action_tx.clone();

        vec![
            MenuItem::Standard(StandardItem {
                label: "Mostrar janela".into(),
                icon_name: "window-new-symbolic".into(),
                activate: Box::new(move |_| {
                    let _ = action_show.send(UiAction::ShowWindow);
                }),
                ..StandardItem::default()
            }),
            MenuItem::Checkmark(CheckmarkItem {
                label: "Webcam virtual".into(),
                enabled: true,
                checked: webcam_on,
                activate: Box::new(move |tray| {
                    let prev = web_state.load(Ordering::Relaxed);
                    web_state.store(!prev, Ordering::Relaxed);
                    let _ = tray.cmd_tx.send(EngineCmd::SetWebcam(!prev));
                    let _ = tray.action_tx.send(UiAction::SetWebcam(!prev));
                }),
                ..CheckmarkItem::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(move |_| {
                    let _ = action_quit.send(UiAction::Quit);
                }),
                ..StandardItem::default()
            }),
        ]
    }
}
