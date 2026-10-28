//! GUI (Fase 3): preview, webcam, painel dinâmico de configurações (CameraWidget
//! tree, fiel ao Entangle), captura + download e galeria da sessão.
//!
//! Padrão (gtk-rs): a GUI lê eventos do core-camera num único timeout_add_local
//! e manda comandos; o frame chega como RGBA e vira `gdk::MemoryTexture`.

use crate::core::camera::{self, CameraEngine, EngineCmd, EngineEv};
use crate::core::config::{CfgKind, CfgNode, CfgValue};
use crossbeam_channel::{Receiver, Sender};
use gtk4::prelude::*;
#[allow(deprecated)]
use gtk4::{gdk, glib, Align, ButtonsType, MessageDialog};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub const APP_ID: &str = "br.lenvix";

/// Ações que vêm da tray (thread separada) e precisam rodar na thread GTK
#[derive(Debug)]
pub enum UiAction {
    ShowWindow,
    SetWebcam(bool),
    Quit,
}

struct Widgets {
    picture: gtk4::Picture,
    lbl_model: gtk4::Label,
    lbl_status: gtk4::Label,
    lbl_webctr: gtk4::Label,
    lbl_fps: gtk4::Label,
    sw_webcam: gtk4::Switch,
    sw_mirror: gtk4::Switch,
    spinner: gtk4::SpinButton,
    zoom_scale: gtk4::Scale,
    panx_scale: gtk4::Scale,
    pany_scale: gtk4::Scale,
    #[allow(deprecated)] // ComboBoxText 4.10: alternativa em julgamento para fase 4
    out_combo: gtk4::ComboBoxText,
    btn_connect: gtk4::Button,
    sw_autostart: gtk4::Switch,
    btn_capture: gtk4::Button,
    #[allow(deprecated)]
    cam_combo: gtk4::ComboBoxText,
    btn_liveview: gtk4::Button,
    #[allow(deprecated)]
    dev_combo: gtk4::ComboBoxText,
    lbl_session: gtk4::Label,
    gal_box: gtk4::Box,
    config_box: gtk4::Box,
    config_scroll: gtk4::ScrolledWindow,
}

pub fn activate(
    app: &gtk4::Application,
    engine: Rc<RefCell<CameraEngine>>,
    action_rx: Receiver<UiAction>,
) {
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Lenvix")
        .default_width(1280)
        .default_height(760)
        .resizable(true)
        .build();

    let root = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    // ------- linha principal: controles | preview | config -------
    let mid = gtk4::Box::builder().orientation(gtk4::Orientation::Horizontal).spacing(8).build();

    // painel de controles (FR-05/FR-06/FR-20/FR-22)
    let left = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .width_request(300)
        .valign(Align::Start)
        .build();

    let lbl_model = gtk4::Label::builder().label("carregando…").halign(Align::Start).build();
    let lbl_status = gtk4::Label::builder().label("iniciando…").halign(Align::Start).wrap(true).build();
    let lbl_webctr = gtk4::Label::builder().label("webcam: iniciando…").halign(Align::Start).build();

    let left_frame = gtk4::Frame::new(None);
    left.set_margin_top(8);
    left.set_margin_bottom(8);
    left.set_margin_start(8);
    left.set_margin_end(8);
    left_frame.set_child(Some(&left));

    // --- Seção: Câmera (estilo Entangle) ---
    let sec_cam_lbl = gtk4::Label::builder()
        .label("<b>Câmera</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    #[allow(deprecated)]
    let cam_combo = gtk4::ComboBoxText::new();
    #[allow(deprecated)]
    cam_combo.append_text("Nenhuma câmera detectada");
    #[allow(deprecated)]
    cam_combo.set_active(Some(0));

    let btn_connect = gtk4::Button::with_label("Conectar câmera");
    let btn_liveview = gtk4::Button::with_label("Live View");
    btn_liveview.set_sensitive(false);

    let btn_cam_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .homogeneous(true)
        .build();
    btn_cam_row.append(&btn_connect);
    btn_cam_row.append(&btn_liveview);

    let btn_capture = gtk4::Button::with_label("Capturar (full-res)");
    btn_capture.set_tooltip_text(Some("captura no cartão e baixa para a sessão local"));
    btn_capture.set_sensitive(false);

    let sw_autostart = gtk4::Switch::builder()
        .halign(Align::End)
        .valign(Align::Center)
        .build();
    let auto_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    let lbl_auto = gtk4::Label::new(Some("Auto-conectar USB"));
    lbl_auto.set_halign(Align::Start);
    lbl_auto.set_hexpand(true);
    auto_row.append(&lbl_auto);
    auto_row.append(&sw_autostart);

    // --- Seção: Webcam Virtual ---
    let sec_web_lbl = gtk4::Label::builder()
        .label("<b>Webcam Virtual</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let sw_webcam = gtk4::Switch::builder()
        .halign(Align::End)
        .valign(Align::Center)
        .hexpand(false)
        .build();
    let lbl_webcam = gtk4::Label::builder()
        .label("Webcam virtual")
        .halign(Align::Start)
        .hexpand(true)
        .build();
    let webcam_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    webcam_row.append(&lbl_webcam);
    webcam_row.append(&sw_webcam);

    #[allow(deprecated)]
    let dev_combo = gtk4::ComboBoxText::with_entry();
    #[allow(deprecated)]
    dev_combo.append_text("/dev/video9");
    #[allow(deprecated)]
    dev_combo.set_active(Some(0));

    let btn_add_dev = gtk4::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Criar nova saída virtual (/dev/videoN)")
        .build();

    let dev_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(4)
        .build();
    dev_combo.set_hexpand(true);
    dev_box.append(&dev_combo);
    dev_box.append(&btn_add_dev);
    let dev_row = ui_row("Dispositivo", &dev_box);

    #[allow(deprecated)]
    let out_combo = gtk4::ComboBoxText::new();
    for o in [
        "Nativa (sem crop)",
        "1280×720 — 16:9",
        "1920×1080 — 16:9",
        "960×540 — 16:9",
        "720×1280 — 9:16",
        "1080×1920 — 9:16",
        "1080×1080 — 1:1",
        "1440×1080 — 4:3",
        "1080×1440 — 3:4",
    ] {
        #[allow(deprecated)]
        out_combo.append_text(o);
    }
    #[allow(deprecated)]
    out_combo.set_active(Some(2));
    let out_row = ui_row("Resolução", &out_combo);

    let spinner = gtk4::SpinButton::with_range(5.0, 60.0, 1.0);
    spinner.set_value(30.0);
    let fps_row = ui_row("FPS alvo", &spinner);

    // --- Seção: Ajustes de Imagem ---
    let sec_adj_lbl = gtk4::Label::builder()
        .label("<b>Ajustes de Imagem</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let sw_mirror = gtk4::Switch::builder()
        .halign(Align::End)
        .valign(Align::Center)
        .hexpand(false)
        .build();
    let lbl_mirror_row = gtk4::Label::builder()
        .label("Espelhar imagem")
        .halign(Align::Start)
        .hexpand(true)
        .build();
    let mirror_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    mirror_row.append(&lbl_mirror_row);
    mirror_row.append(&sw_mirror);

    let mk_scale = |initial: f64, min: f64, max: f64, step: f64, digits: i32| {
        let adj = gtk4::Adjustment::new(initial, min, max, step, step * 4.0, 0.0);
        let sc = gtk4::Scale::builder()
            .adjustment(&adj)
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .digits(digits)
            .build();
        sc.set_size_request(120, -1);
        sc
    };
    let zoom_scale = mk_scale(1.0, 1.0, 4.0, 0.05, 2);
    let panx_scale = mk_scale(0.0, -100.0, 100.0, 1.0, 0);
    let pany_scale = mk_scale(0.0, -100.0, 100.0, 1.0, 0);

    let zoom_row = ui_row("Zoom", &zoom_scale);
    let panx_row = ui_row("Pan X", &panx_scale);
    let pany_row = ui_row("Pan Y", &pany_scale);

    let sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    let sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);

    // Monta o painel esquerdo
    left.append(&sec_cam_lbl);
    left.append(&cam_combo);
    left.append(&btn_cam_row);
    left.append(&btn_capture);
    left.append(&auto_row);
    left.append(&lbl_status);

    left.append(&sep1);
    left.append(&sec_web_lbl);
    left.append(&webcam_row);
    left.append(&dev_row);
    left.append(&out_row);
    left.append(&fps_row);
    left.append(&lbl_webctr);

    left.append(&sep2);
    left.append(&sec_adj_lbl);
    left.append(&mirror_row);
    left.append(&zoom_row);
    left.append(&panx_row);
    left.append(&pany_row);

    // guard: enquanto a GUI aplica o AppState salvo (engine), os handlers não
    // devem reenviar os mesmos comandos (eco)
    let suppress = Rc::new(std::cell::Cell::new(false));

    let tx = engine.borrow().cmd_tx.clone();
    let suppress_zoom = suppress.clone();
    zoom_scale.connect_value_changed(move |z| {
        if suppress_zoom.get() {
            return;
        }
        let _ = tx.send(EngineCmd::SetZoom(z.value() as f32));
    });
    let tx = engine.borrow().cmd_tx.clone();
    let suppress_panx = suppress.clone();
    panx_scale.connect_value_changed(move |z| {
        if suppress_panx.get() {
            return;
        }
        let _ = tx.send(EngineCmd::SetZoomPos { x: Some(z.value() as i16), y: None });
    });
    let tx = engine.borrow().cmd_tx.clone();
    let suppress_pany = suppress.clone();
    pany_scale.connect_value_changed(move |z| {
        if suppress_pany.get() {
            return;
        }
        let _ = tx.send(EngineCmd::SetZoomPos { x: None, y: Some(z.value() as i16) });
    });
    let tx = engine.borrow().cmd_tx.clone();
    let suppress_out = suppress.clone();
    #[allow(deprecated)]
    out_combo.connect_changed(move |cb| {
        if suppress_out.get() {
            return;
        }
        #[allow(deprecated)]
        let out = match cb.active() {
            Some(1) => Some((1280, 720)),
            Some(2) => Some((1920, 1080)),
            Some(3) => Some((960, 540)),
            Some(4) => Some((720, 1280)),
            Some(5) => Some((1080, 1920)),
            Some(6) => Some((1080, 1080)),
            Some(7) => Some((1440, 1080)),
            Some(8) => Some((1080, 1440)),
            _ => None,
        };
        let _ = tx.send(EngineCmd::SetOutSize(out));
    });

    // FPS alvo
    let cmd_tx = engine.borrow().cmd_tx.clone();
    let suppress_fps = suppress.clone();
    spinner.connect_value_changed(move |s| {
        if suppress_fps.get() {
            return;
        }
        let _ = cmd_tx.send(EngineCmd::SetFps(s.value() as u32));
    });

    // FR-22: toggle webcam
    let cmd_tx = engine.borrow().cmd_tx.clone();
    sw_webcam.connect_state_set(move |_sw, state| {
        let _ = cmd_tx.send(EngineCmd::SetWebcam(state));
        glib::Propagation::Proceed
    });

    // conecta/desconecta a câmera (SetConnected)
    let cmd_tx = engine.borrow().cmd_tx.clone();
    let cam_on = Rc::new(std::cell::Cell::new(false));
    let cam_on_btn = cam_on.clone();
    btn_connect.connect_clicked(move |_| {
        let _ = cmd_tx.send(EngineCmd::SetConnected(!cam_on_btn.get()));
    });
    sw_autostart.connect_state_set({
        let cmd_tx = engine.borrow().cmd_tx.clone();
        move |_sw, state| {
            let _ = cmd_tx.send(EngineCmd::SetAutostart(state));
            glib::Propagation::Proceed
        }
    });

    // captura
    let cmd_tx = engine.borrow().cmd_tx.clone();
    btn_capture.connect_clicked(move |_| {
        let _ = cmd_tx.send(EngineCmd::Capture);
    });

    // espelhar
    let cmd_tx = engine.borrow().cmd_tx.clone();
    let suppress_mirror = suppress.clone();
    sw_mirror.connect_state_set(move |_sw, state| {
        if suppress_mirror.get() {
            return glib::Propagation::Proceed;
        }
        let _ = cmd_tx.send(EngineCmd::SetMirror(state));
        glib::Propagation::Proceed
    });

    // liveview toggle (estilo Entangle)
    let cmd_tx = engine.borrow().cmd_tx.clone();
    let live_on = Rc::new(std::cell::Cell::new(false));
    let live_on_btn = live_on.clone();
    btn_liveview.connect_clicked(move |_| {
        let _ = cmd_tx.send(EngineCmd::SetLiveView(!live_on_btn.get()));
    });

    // seleção de câmera (cam_combo)
    let cmd_tx = engine.borrow().cmd_tx.clone();
    let suppress_cam = suppress.clone();
    let cameras_data = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    let cameras_data_combo = cameras_data.clone();
    #[allow(deprecated)]
    cam_combo.connect_changed(move |cb| {
        if suppress_cam.get() {
            return;
        }
        if let Some(ix) = cb.active()
            && let Some((model, port)) = cameras_data_combo.borrow().get(ix as usize)
        {
            let _ = cmd_tx.send(EngineCmd::Reselect { model: model.clone(), port: port.clone() });
        }
    });

    // seleção de dispositivo loopback (dev_combo + btn_add_dev)
    {
        let cmd_tx = engine.borrow().cmd_tx.clone();
        btn_add_dev.connect_clicked(move |_| {
            let _ = cmd_tx.send(EngineCmd::CreateNewVideoDevice);
        });
    }

    fn parse_video_nr(text: &str) -> Option<u32> {
        let s = text.trim();
        if let Ok(n) = s.parse::<u32>() {
            return Some(n);
        }
        if let Some(rest) = s.strip_prefix("/dev/video") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits.parse::<u32>().ok();
        }
        if let Some(rest) = s.strip_prefix("video") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits.parse::<u32>().ok();
        }
        None
    }

    let cmd_tx = engine.borrow().cmd_tx.clone();
    let suppress_dev = suppress.clone();
    #[allow(deprecated)]
    dev_combo.connect_changed(move |cb| {
        if suppress_dev.get() {
            return;
        }
        if let Some(txt) = cb.active_text()
            && let Some(n) = parse_video_nr(&txt)
        {
            let _ = cmd_tx.send(EngineCmd::SetVideoDevice(n));
        }
    });

    #[allow(deprecated)]
    if let Some(entry) = dev_combo.child().and_then(|w| w.downcast::<gtk4::Entry>().ok()) {
        let cmd_tx = engine.borrow().cmd_tx.clone();
        let suppress_entry = suppress.clone();
        entry.connect_activate(move |e| {
            if suppress_entry.get() {
                return;
            }
            let txt = e.text();
            if let Some(n) = parse_video_nr(&txt) {
                let _ = cmd_tx.send(EngineCmd::SetVideoDevice(n));
            }
        });
    }

    // ------- painel de preview (FR-05) -------
    let picture = gtk4::Picture::builder().content_fit(gtk4::ContentFit::ScaleDown).build();
    let lbl_fps = gtk4::Label::builder()
        .label("fps: –")
        .halign(Align::End)
        .css_classes(["dim-label"])
        .build();
    let frame = gtk4::Frame::new(None);
    frame.set_child(Some(&picture));
    frame.set_hexpand(true);
    frame.set_vexpand(true);

    // ------- painel dinâmico de configurações (esquerda) -------
    let config_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(4)
        .valign(Align::Start)
        .build();
    let config_scroll = gtk4::ScrolledWindow::new();
    config_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    config_scroll.set_child(Some(&config_box));
    config_scroll.set_vexpand(true);
    let right_frame = gtk4::Frame::new(Some("Configurações"));
    right_frame.set_child(Some(&config_scroll));
    right_frame.set_size_request(360, -1);

    mid.append(&right_frame);
    mid.append(&frame);
    mid.append(&left_frame);
    root.append(&mid);

    // ------- galeria da sessão (bottom) -------
    let gal_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    let gal_scroll = gtk4::ScrolledWindow::new();
    gal_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);
    gal_scroll.set_size_request(-1, 132);
    let lbl_session = gtk4::Label::builder()
        .label("")
        .halign(Align::Start)
        .css_classes(["dim-label"])
        .build();
    let btn_open_dir = gtk4::Button::with_label("Abrir pasta");
    let gal_head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    gal_head.append(&lbl_session);
    gal_head.append(&btn_open_dir);
    let gal_inner = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    gal_inner.append(&gal_head);
    gal_inner.append(&gal_scroll);
    let gal_frame = gtk4::Frame::new(Some("Sessão"));
    gal_scroll.set_child(Some(&gal_box));
    gal_frame.set_child(Some(&gal_inner));
    root.append(&gal_frame);

    let w = Widgets {
        picture,
        lbl_model,
        lbl_status,
        lbl_webctr,
        lbl_fps,
        sw_webcam,
        sw_mirror,
        spinner,
        zoom_scale,
        panx_scale,
        pany_scale,
        out_combo,
        btn_connect,
        sw_autostart,
        btn_capture,
        cam_combo,
        btn_liveview,
        dev_combo,
        lbl_session,
        gal_box,
        config_box,
        config_scroll,
    };

    window.set_child(Some(&root));
    window.present();

    // FR-26: fechar a janela ≠ sair do app (continua em segundo plano na tray)
    window.connect_close_request({
        let window = window.clone();
        move |_| {
            window.set_visible(false);
            glib::Propagation::Stop
        }
    });

    // sessão: caminho mostrado desde já + abrir pasta; galeria carregada no startup
    let dir = camera::session_dir();
    w.lbl_session.set_text(&format!("fotos salvas em: {}", dir.display()));
    let dopen = dir.clone();
    btn_open_dir.connect_clicked(move |_| {
        let _ = std::process::Command::new("xdg-open").arg(&dopen).spawn();
    });
    refresh_gallery(&w);

    // loop de eventos (única fonte de verdade: core-camera + tray)
    poll_events(app, &window, engine, action_rx, w, cam_on, live_on, cameras_data, suppress);
}

// ------------ painel dinâmico de configurações -------------------------------

fn render_config(box_: &gtk4::Box, nodes: &[CfgNode], cmd: &Sender<EngineCmd>) {
    while let Some(c) = box_.first_child() {
        box_.remove(&c);
    }
    for n in nodes {
        add_node(box_, n, cmd);
    }
}

fn row(host: &gtk4::Box, tooltip: &str, widget: &impl IsA<gtk4::Widget>) {
    if !tooltip.is_empty() {
        widget.as_ref().set_tooltip_text(Some(tooltip));
    }
    host.append(widget.as_ref());
}

fn add_node(host: &gtk4::Box, n: &CfgNode, cmd: &Sender<EngineCmd>) {
    match n.kind {
        CfgKind::Section => {
            if has_content(n) {
                let inner = gtk4::Box::builder()
                    .orientation(gtk4::Orientation::Vertical)
                    .spacing(4)
                    .margin_start(8)
                    .margin_top(2)
                    .margin_bottom(4)
                    .build();
                for c in &n.children {
                    add_node(&inner, c, cmd);
                }
                let f = gtk4::Frame::new(Some(&n.label));
                f.set_child(Some(&inner));
                row(host, &n.info, &f);
            }
        }
        CfgKind::Text => {
            if let Some(CfgValue::Text(v)) = &n.value {
                let e = gtk4::Entry::builder().text(v).sensitive(!n.readonly).build();
                let cmd = cmd.clone();
                let name = n.name.clone();
                e.connect_activate(move |e| {
                    let _ = cmd.send(EngineCmd::SetConfig { name: name.clone(), value: e.text().into() });
                });
                host.append(&kv_row(&e, &n.label, &n.info));
            }
        }
        CfgKind::Range => {
            if let Some(CfgValue::Range { value, min, max, step }) = &n.value {
                let sb = gtk4::SpinButton::with_range(f64::from(*min), f64::from(*max), nonzero_step(*step, *max));
                sb.set_digits(digits_of(*step, *max) as u32);
                sb.set_value(f64::from(*value));
                sb.set_sensitive(!n.readonly);
                let cmd = cmd.clone();
                let name = n.name.clone();
                sb.connect_value_changed(move |sb| {
                    let _ = cmd.send(EngineCmd::SetConfig {
                        name: name.clone(),
                        value: format!("{}", sb.value()),
                    });
                });
                host.append(&kv_row(&sb, &n.label, &n.info));
            }
        }
        CfgKind::Toggle => {
            if let Some(CfgValue::Toggle(v)) = &n.value {
                let sw = gtk4::Switch::default();
                sw.set_state(*v);
                sw.set_sensitive(!n.readonly);
                sw.set_halign(Align::Start);
                let cmd = cmd.clone();
                let name = n.name.clone();
                sw.connect_state_set(move |_sw, state| {
                    let _ = cmd.send(EngineCmd::SetConfig {
                        name: name.clone(),
                        value: if state { "1" } else { "0" }.into(),
                    });
                    glib::Propagation::Proceed
                });
                host.append(&kv_row(&sw, &n.label, &n.info));
            }
        }
        CfgKind::Radio => {
            if let Some(CfgValue::Radio { options, current }) = &n.value {
                #[allow(deprecated)]
                let cb = gtk4::ComboBoxText::new();
                for o in options {
                    #[allow(deprecated)]
                    cb.append_text(o);
                }
                #[allow(deprecated)]
                cb.set_active(options.iter().position(|c| c == current).map(|i| i as u32));
                cb.set_sensitive(!n.readonly);
                let cmd = cmd.clone();
                let name = n.name.clone();
                let opts = options.clone();
                #[allow(deprecated)]
                cb.connect_changed(move |cb| {
                    if let Some(ix) = cb.active() {
                        let _ = cmd.send(EngineCmd::SetConfig {
                            name: name.clone(),
                            value: opts
                                .get(ix as usize)
                                .cloned()
                                .unwrap_or_default(),
                        });
                    }
                });
                host.append(&kv_row(&cb, &n.label, &n.info));
            }
        }
        CfgKind::Button => {
            let b = gtk4::Button::with_label(&n.label);
            let cmd = cmd.clone();
            let name = n.name.clone();
            b.connect_clicked(move |_| {
                let _ = cmd.send(EngineCmd::PressButton { name: name.clone() });
            });
            row(host, &n.info, &b);
        }
        CfgKind::Date => {} // não exposto na UI (raro e inútil na câmera)§
    }
}


/// Linha do painel de config: input à esquerda, label (wrap) à direita
fn kv_row(input: &impl IsA<gtk4::Widget>, label_text: &str, info: &str) -> gtk4::Box {
    let r = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .valign(Align::Start)
        .build();
    input.as_ref().set_valign(Align::Center);
    let l = gtk4::Label::builder()
        .label(label_text)
        .halign(Align::Start)
        .valign(Align::Center)
        .wrap(true)
        .wrap_mode(gtk4::pango::WrapMode::WordChar)
        .max_width_chars(26)
        .xalign(0.0)
        .build();
    r.append(input.as_ref());
    r.append(&l);
    if !info.is_empty() {
        r.set_tooltip_text(Some(info));
    }
    r
}

fn nonzero_step(step: f32, max: f32) -> f64 {
    if step > f32::EPSILON {
        f64::from(step)
    } else {
        f64::from(max).max(1.0) / 100.0
    }
}

fn digits_of(step: f32, _max: f32) -> i32 {
    if step > f32::EPSILON {
        let mut s = step;
        let mut d = 0;
        while s > f32::EPSILON && s.fract() > f32::EPSILON && d < 4 {
            s *= 10.0;
            d += 1;
        }
        d
    } else {
        2 // fallback: 2 casas de precisão
    }
}

fn has_content(n: &CfgNode) -> bool {
    !n.children.is_empty()
}

// ------------ galeria da sessão -------------------------------------

fn refresh_gallery(w: &Widgets) {
    while let Some(c) = w.gal_box.first_child() {
        w.gal_box.remove(&c);
    }
    let dir = camera::session_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|x| x.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
                Some("jpg" | "jpeg" | "cr2" | "cr3" | "png")
            )
        })
        .filter_map(|p| {
            p.metadata().ok().map(|m| (p, m.modified().ok().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect();
    files.sort_by_key(|fa| std::cmp::Reverse(fa.1));
    if files.is_empty() {
        let l = gtk4::Label::builder().label("(sem fotos)".to_string()).build();
        l.set_sensitive(false);
        w.gal_box.append(&l);
    }
    for (p, _) in files.iter().take(24) {
        let btn = gtk4::Button::new();
        let img = gtk4::Image::builder().pixel_size(96).build();
        let ext = p.extension().and_then(|x| x.to_str()).map(|s| s.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png")) {
            img.set_from_file(Some(p));
        } else {
            // RAW (CR2/CR3): GTK não decodifica — ícone genérico
            img.set_icon_name(Some("image-x-generic"));
        }
        btn.set_child(Some(&img));
        let size = p.metadata().map(|m| m.len()).unwrap_or(0);
        btn.set_tooltip_text(Some(&format!(
            "{} ({:.1} MB)",
            p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            size as f64 / 1e6
        )));
        let path = p.clone();
        btn.connect_clicked(move |_| {
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        });
        w.gal_box.append(&btn);
    }
}

// ------------ loop de eventos --------------------------------------

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn poll_events(
    app: &gtk4::Application,
    window: &gtk4::ApplicationWindow,
    engine: Rc<RefCell<CameraEngine>>,
    action_rx: Receiver<UiAction>,
    w: Widgets,
    cam_on: Rc<std::cell::Cell<bool>>,
    live_on: Rc<std::cell::Cell<bool>>,
    cameras_data: Rc<RefCell<Vec<(String, String)>>>,
    suppress: Rc<std::cell::Cell<bool>>,
) {
    let app = app.clone();
    let engine = engine.clone();
    let window = window.clone();
    let ev_rx = engine.borrow().ev_rx.clone();

    let pending_scroll = Rc::new(std::cell::Cell::new(None::<f64>));
    let ps_notify = pending_scroll.clone();
    w.config_scroll.vadjustment().connect_upper_notify(move |adj| {
        if let Some(target) = ps_notify.take() {
            adj.set_value(target);
        }
    });

    glib::timeout_add_local(Duration::from_millis(20), move || {
        // ações da tray (delegadas para a thread GTK)
        for act in action_rx.try_iter() {
            match act {
                UiAction::ShowWindow => window.set_visible(true),
                UiAction::SetWebcam(on) => {
                    let _ = engine.borrow().cmd_tx.send(EngineCmd::SetWebcam(on));
                    w.sw_webcam.set_active(on);
                }
                UiAction::Quit => {
                    engine.borrow_mut().shutdown();
                    app.quit();
                    return glib::ControlFlow::Break;
                }
            }
        }

        // eventos do core-camera
        for ev in ev_rx.try_iter() {
            match ev {
                EngineEv::CamerasList { cameras, active } => {
                    *cameras_data.borrow_mut() = cameras.clone();
                    suppress.set(true);
                    #[allow(deprecated)]
                    w.cam_combo.remove_all();
                    if cameras.is_empty() {
                        #[allow(deprecated)]
                        w.cam_combo.append_text("Nenhuma câmera detectada");
                        #[allow(deprecated)]
                        w.cam_combo.set_active(Some(0));
                        w.btn_connect.set_sensitive(false);
                        w.lbl_model.set_text("câmera não conectada");
                        if !cam_on.get() {
                            w.lbl_status.set_text("conecte a câmera via USB");
                        }
                    } else {
                        let mut active_idx = None;
                        for (i, (model, port)) in cameras.iter().enumerate() {
                            let label = format!("{model} [{port}]");
                            #[allow(deprecated)]
                            w.cam_combo.append_text(&label);
                            if model == &active.0 && port == &active.1 {
                                active_idx = Some(i as u32);
                            }
                        }
                        if active_idx.is_none() {
                            active_idx = Some(0);
                        }
                        #[allow(deprecated)]
                        w.cam_combo.set_active(active_idx);
                        w.btn_connect.set_sensitive(true);
                        w.lbl_model.set_text(&active.0);
                        if !cam_on.get() {
                            w.lbl_status.set_text(&format!(
                                "{} câmera(s) detectada(s) — clique em Conectar câmera",
                                cameras.len(),
                            ));
                        }
                    }
                    suppress.set(false);
                }
                EngineEv::DevicesList { devices, active } => {
                    suppress.set(true);
                    #[allow(deprecated)]
                    w.dev_combo.remove_all();
                    let mut active_idx = None;
                    let mut active_found = false;
                    for (i, dev) in devices.iter().enumerate() {
                        let status_str = if dev.nr == active {
                            "ativa"
                        } else if dev.in_use {
                            "ocupado / indisponível"
                        } else {
                            "disponível"
                        };
                        let label = format!("/dev/video{} ({} — {})", dev.nr, dev.card, status_str);
                        #[allow(deprecated)]
                        w.dev_combo.append_text(&label);
                        if dev.nr == active {
                            active_idx = Some(i as u32);
                            active_found = true;
                        }
                    }
                    if !active_found {
                        let custom = format!("/dev/video{active} (ativa)");
                        #[allow(deprecated)]
                        w.dev_combo.append_text(&custom);
                        #[allow(deprecated)]
                        w.dev_combo.set_active(Some(devices.len() as u32));
                    } else {
                        #[allow(deprecated)]
                        w.dev_combo.set_active(active_idx);
                    }
                    suppress.set(false);
                }
                EngineEv::LiveViewStatus(active) => {
                    live_on.set(active);
                    if active {
                        w.btn_liveview.set_label("Parar Live View");
                        w.btn_liveview.add_css_class("destructive-action");
                        w.btn_liveview.remove_css_class("suggested-action");
                        w.lbl_status.set_text("Live View ativo (preview contínuo)");
                    } else {
                        w.btn_liveview.set_label("Iniciar Live View");
                        w.btn_liveview.add_css_class("suggested-action");
                        w.btn_liveview.remove_css_class("destructive-action");
                        if cam_on.get() {
                            w.lbl_status.set_text("câmera conectada (obturador fechado)");
                        }
                    }
                }
                EngineEv::CameraOpened { model } => {
                    w.lbl_model.set_text(&model);
                    w.lbl_status.set_text("conectada (obturador fechado)");
                }
                EngineEv::Frame { w: fw, h: fh, rgba, measured_fps } => {
                    let stride = (fw * 4) as usize;
                    let bytes = glib::Bytes::from_owned(rgba);
                    let tex = gdk::MemoryTexture::new(
                        fw as i32,
                        fh as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &bytes,
                        stride,
                    );
                    w.picture.set_paintable(Some(&tex));
                    w.lbl_fps.set_text(&format!("fps: {measured_fps:.1} — {fw}×{fh}"));
                }
                EngineEv::AppState { fps, webcam, mirror, zoom, pan_x, pan_y, out, autoconnect, video_device: _, liveview } => {
                    suppress.set(true);
                    w.spinner.set_value(f64::from(fps));
                    w.sw_webcam.set_active(webcam);
                    w.sw_mirror.set_active(mirror);
                    w.sw_autostart.set_active(autoconnect);
                    w.zoom_scale.set_value(f64::from(zoom));
                    w.panx_scale.set_value(f64::from(pan_x));
                    w.pany_scale.set_value(f64::from(pan_y));
                    #[allow(deprecated)]
                    w.out_combo.set_active(match out {
                        Some((1280, 720)) => Some(1),
                        Some((1920, 1080)) => Some(2),
                        Some((960, 540)) => Some(3),
                        Some((720, 1280)) => Some(4),
                        Some((1080, 1920)) => Some(5),
                        Some((1080, 1080)) => Some(6),
                        Some((1440, 1080)) => Some(7),
                        Some((1080, 1440)) => Some(8),
                        _ => Some(0),
                    });
                    live_on.set(liveview);
                    suppress.set(false);
                }
                EngineEv::Connected(on) => {
                    cam_on.set(on);
                    w.btn_connect.set_label(if on { "Desconectar câmera" } else { "Conectar câmera" });
                    w.btn_liveview.set_sensitive(on);
                    w.btn_capture.set_sensitive(on);
                    if on {
                        w.lbl_status.set_text("câmera conectada (obturador em repouso — inicie o Live View para preview)");
                    } else {
                        live_on.set(false);
                        w.btn_liveview.set_label("Live View");
                        w.btn_liveview.remove_css_class("destructive-action");
                        w.btn_liveview.remove_css_class("suggested-action");
                        w.lbl_status.set_text("standby — conecte a câmera para preview");
                        w.lbl_model.set_text("câmera não conectada");
                    }
                }
                EngineEv::PreviewLost(e) => {
                    w.lbl_status.set_markup(&format!("<b>preview: {e}</b>"));
                }
                EngineEv::WebcamStatus { on, path, card, error } => {
                    w.sw_webcam.set_active(on);
                    match (&path, &error) {
                        (Some(_), None) => {
                            let nome = card.clone().unwrap_or_else(|| "ativa".to_string());
                            w.lbl_webctr.set_text(&format!("webcam: {nome}"));
                            w.lbl_status.set_text(&format!("webcam ativa — {nome}"));
                        }
                        (_, Some(e)) => w.lbl_status.set_markup(&format!("<b>{e}</b>")),
                        (None, None) => {
                            w.lbl_webctr.set_text("webcam: desligada");
                            w.lbl_status.set_text("câmera conectada");
                        }
                    }
                }
                EngineEv::ConfigTree(nodes) => {
                    let cmd = engine.borrow().cmd_tx.clone();
                    let adj = w.config_scroll.vadjustment();
                    let pos = adj.value();
                    if pos > 0.0 {
                        pending_scroll.set(Some(pos));
                    }
                    render_config(&w.config_box, &nodes, &cmd);
                    let adj_c = adj.clone();
                    let ps_idle = pending_scroll.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(target) = ps_idle.take() {
                            adj_c.set_value(target);
                        }
                    });
                }
                EngineEv::CaptureBusy => {
                    w.btn_capture.set_sensitive(false);
                    w.lbl_status.set_text("captura em execução…");
                }
                EngineEv::CaptureDone { path, error } => {
                    w.btn_capture.set_sensitive(true);
                    match (path, error) {
                        (Some(p), _) => {
                            w.lbl_status.set_text(&format!("foto salva: {p}"));
                            refresh_gallery(&w);
                        }
                        (None, Some(e)) => {
                            w.lbl_status.set_markup(&format!("<b>{e}</b>"));
                        }
                        (None, None) => {}
                    }
                }
                EngineEv::Fatal(e) => dlg_fatal(window.upcast_ref(), &e),
            }
        }

        glib::ControlFlow::Continue
    });
}

#[allow(deprecated)]
fn dlg_fatal(parent: &gtk4::Window, msg: &str) {
    let dlg = MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .buttons(ButtonsType::Close)
        .text("Lenvix — erro")
        .secondary_text(msg)
        .build();
    dlg.show();
}

/// Linha label+widget do painel "Câmera"
fn ui_row(label: &str, w: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let r = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    let l = gtk4::Label::new(Some(label));
    l.set_valign(Align::Center);
    r.append(&l);
    r.append(w.as_ref());
    r
}
