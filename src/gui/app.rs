//! GUI (Fase 3): preview, webcam, painel dinâmico de configurações (CameraWidget
//! tree, fiel ao Entangle), captura + download e galeria da sessão.
//!
//! Padrão (gtk-rs): a GUI lê eventos do core-camera num único timeout_add_local
//! e manda comandos; o frame chega como RGBA e vira `gdk::MemoryTexture`.

use crate::core::camera::{self, CameraEngine, EngineCmd, EngineEv};
use crate::core::config::{CfgKind, CfgNode, CfgValue};
use crate::core::settings;
use crossbeam_channel::{Receiver, Sender};
use gtk4::prelude::*;
#[allow(deprecated)]
use gtk4::{gdk, glib, Align, ButtonsType, MessageDialog};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub const APP_ID: &str = "br.lenvix";

/// Catálogo de controles da câmera selecionáveis na tela de configurações.
/// (chave persistida, rótulo, aliases de nome/label dos widgets do gphoto2).
/// Apenas os marcados são exibidos no painel "Configurações" (ordem = ordem da UI).
pub const CONTROLS_CATALOG: &[(&str, &str, &[&str])] = &[
    ("iso", "ISO", &["iso", "isospeed", "isospeedrate"]),
    ("shutterspeed", "Velocidade (obturador)", &["shutterspeed", "shutterspeed2", "shutter"]),
    ("aperture", "Abertura", &["aperture", "f-number"]),
    ("whitebalance", "Balanço de branco", &["whitebalance"]),
    ("exposurecompensation", "Compensação de exposição", &["exposurecompensation", "exposurecomp"]),
    ("focusmode", "Modo de foco", &["focusmode", "focusmode2"]),
    ("imageformat", "Formato de imagem", &["imageformat", "imagequality", "imagesize"]),
    ("flashmode", "Modo de flash", &["flashmode", "flashmode2", "flash"]),
    ("meteringmode", "Fotometria", &["meteringmode", "exposuremeteringmode", "aemode"]),
    ("exposureprogrammode", "Modo de programa (P/A/S/M)", &["exposureprogrammode", "autoexposuremode"]),
    ("capturetarget", "Destino de captura", &["capturetarget", "capturedestination"]),
];

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
    btn_record: gtk4::Button,
    #[allow(deprecated)]
    cam_combo: gtk4::ComboBoxText,
    #[allow(deprecated)]
    audio_combo: gtk4::ComboBoxText,
    #[allow(deprecated)]
    vfmt_combo: gtk4::ComboBoxText,
    lbl_scan_status: gtk4::Label,
    btn_liveview: gtk4::Button,
    #[allow(deprecated)]
    dev_combo: gtk4::ComboBoxText,
    lbl_session: gtk4::Label,
    gal_box: gtk4::Box,
    config_box: gtk4::Box,
    config_scroll: gtk4::ScrolledWindow,
    window: gtk4::ApplicationWindow,
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

    let header = gtk4::HeaderBar::new();
    header.set_show_title_buttons(true);
    let title_lbl = gtk4::Label::builder()
        .label("<b>Lenvix</b>")
        .use_markup(true)
        .build();
    header.set_title_widget(Some(&title_lbl));

    let btn_settings = gtk4::Button::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Configurações do aplicativo")
        .build();
    header.pack_end(&btn_settings);
    window.set_titlebar(Some(&header));

    let lbl_model = gtk4::Label::builder()
        .label("câmera não conectada")
        .halign(Align::Start)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();
    let lbl_status = gtk4::Label::builder()
        .label("iniciando…")
        .halign(Align::Start)
        .wrap(true)
        .wrap_mode(gtk4::pango::WrapMode::WordChar)
        .max_width_chars(32)
        .xalign(0.0)
        .build();
    let lbl_webctr = gtk4::Label::builder()
        .label("webcam: iniciando…")
        .halign(Align::Start)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .max_width_chars(32)
        .xalign(0.0)
        .build();

    let left_frame = gtk4::Frame::new(None);
    left_frame.set_size_request(320, -1);
    left_frame.set_hexpand(false);
    left.set_size_request(320, -1);
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

    let btn_settings_cam = gtk4::Button::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Configurações e seleção de câmera")
        .build();

    let cam_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    lbl_model.set_hexpand(true);
    cam_box.append(&lbl_model);
    cam_box.append(&btn_settings_cam);

    #[allow(deprecated)]
    let cam_combo = gtk4::ComboBoxText::new();
    #[allow(deprecated)]
    cam_combo.append_text("Nenhuma câmera detectada");
    #[allow(deprecated)]
    cam_combo.set_active(Some(0));

    // --- Janela de Configurações ---
    let settings_win = gtk4::Window::builder()
        .title("Configurações — Lenvix")
        .transient_for(&window)
        .modal(true)
        .default_width(540)
        .default_height(620)
        .resizable(true)
        .build();

    let set_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let set_sec_cam = gtk4::Label::builder()
        .label("<b>Seleção de Câmera</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let set_cam_desc = gtk4::Label::builder()
        .label("Escolha qual câmera DSLR ou mirrorless conectada via USB deve ser utilizada:")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let btn_refresh_cams = gtk4::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Buscar câmeras conectadas agora")
        .build();

    let cam_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .build();
    cam_combo.set_hexpand(true);
    cam_row.append(&cam_combo);
    cam_row.append(&btn_refresh_cams);

    let lbl_scan_status = gtk4::Label::builder()
        .label("Abra esta tela para detectar câmeras conectadas.")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let set_sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);

    // --- Seção Gravação (Áudio e Formato) ---
    let set_sec_rec = gtk4::Label::builder()
        .label("<b>Gravação de Vídeo &amp; Áudio</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let set_rec_desc = gtk4::Label::builder()
        .label("Configure o microfone/dispositivo de áudio e o formato de arquivo para gravações:")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    #[allow(deprecated)]
    let audio_combo = gtk4::ComboBoxText::new();
    #[allow(deprecated)]
    audio_combo.append_text("Padrão do Sistema (default)");
    #[allow(deprecated)]
    audio_combo.append_text("Sem áudio (mudo)");
    #[allow(deprecated)]
    audio_combo.set_active(Some(0));

    let audio_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .build();
    let lbl_audio = gtk4::Label::builder()
        .label("Entrada de áudio:")
        .halign(Align::Start)
        .build();
    audio_combo.set_hexpand(true);
    audio_row.append(&lbl_audio);
    audio_row.append(&audio_combo);

    #[allow(deprecated)]
    let vfmt_combo = gtk4::ComboBoxText::new();
    #[allow(deprecated)]
    vfmt_combo.append_text("MP4 (.mp4 — H.264 + AAC)");
    #[allow(deprecated)]
    vfmt_combo.append_text("MKV (.mkv — H.264 + AAC)");
    #[allow(deprecated)]
    vfmt_combo.append_text("WebM (.webm — VP9 + Opus)");
    #[allow(deprecated)]
    vfmt_combo.append_text("MOV (.mov — QuickTime)");
    #[allow(deprecated)]
    vfmt_combo.append_text("AVI (.avi — MPEG4 + MP3)");
    #[allow(deprecated)]
    vfmt_combo.set_active(Some(0));

    let vfmt_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .build();
    let lbl_vfmt = gtk4::Label::builder()
        .label("Formato do vídeo:")
        .halign(Align::Start)
        .build();
    vfmt_combo.set_hexpand(true);
    vfmt_row.append(&lbl_vfmt);
    vfmt_row.append(&vfmt_combo);

    let set_sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);

    // --- Seção: Controles da câmera exibidos no painel ---
    let set_sec_ctrl = gtk4::Label::builder()
        .label("<b>Controles da Câmera</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let set_ctrl_desc = gtk4::Label::builder()
        .label("Marque os controles que devem aparecer no painel “Configurações”. Itens não marcados ficam ocultos (spam de controles não usados). Os mais usados já vêm marcados por padrão:")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let ctrl_grid = gtk4::Grid::builder()
        .column_spacing(14)
        .row_spacing(6)
        .halign(Align::Start)
        .margin_start(8)
        .build();

    // estado compartilhado: chaves de controles visíveis (settings.conf | GUI)
    let visible_state: Rc<RefCell<Vec<String>>> =
        Rc::new(RefCell::new(crate::core::settings::load().visible_controls));
    // última árvore de configuração recebida da câmera — usada para listar na
    // tela de configurações TODOS os controles que ela realmente expõe
    let last_tree: Rc<RefCell<Vec<CfgNode>>> = Rc::new(RefCell::new(Vec::new()));
    // checkboxes de controles visíveis (recriados a cada abertura das configs)
    let ctrl_checks: Rc<RefCell<Vec<(String, gtk4::CheckButton)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let ctrl_syncing: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(false));

    let set_sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);

    let set_sec_app = gtk4::Label::builder()
        .label("<b>Aplicativo</b>")
        .use_markup(true)
        .halign(Align::Start)
        .build();

    let lbl_ver = gtk4::Label::builder()
        .label(format!("Lenvix v{}", env!("CARGO_PKG_VERSION")))
        .halign(Align::Start)
        .css_classes(["dim-label"])
        .build();

    let btn_close_settings = gtk4::Button::builder()
        .label("Fechar")
        .halign(Align::End)
        .build();

    set_box.append(&set_sec_cam);
    set_box.append(&set_cam_desc);
    set_box.append(&cam_row);
    set_box.append(&lbl_scan_status);
    set_box.append(&set_sep1);
    set_box.append(&set_sec_rec);
    set_box.append(&set_rec_desc);
    set_box.append(&audio_row);
    set_box.append(&vfmt_row);
    set_box.append(&set_sep2);
    set_box.append(&set_sec_ctrl);
    set_box.append(&set_ctrl_desc);
    set_box.append(&ctrl_grid);
    set_box.append(&set_sep3);
    set_box.append(&set_sec_app);
    set_box.append(&lbl_ver);
    set_box.append(&btn_close_settings);

    let set_scroll = gtk4::ScrolledWindow::new();
    set_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let set_scroll_child = set_box.clone();
    set_scroll.set_child(Some(&set_scroll_child));
    settings_win.set_child(Some(&set_scroll));

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

    let btn_capture = gtk4::Button::with_label("Foto (full-res)");
    btn_capture.set_tooltip_text(Some("captura no cartão e baixa para a sessão local"));
    btn_capture.set_sensitive(false);

    let btn_record = gtk4::Button::with_label("Gravar vídeo");
    btn_record.set_tooltip_text(Some("Gravar stream de vídeo ao vivo (MP4/H.264)"));
    btn_record.set_sensitive(false);

    let btn_cap_row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .homogeneous(true)
        .build();
    btn_cap_row.append(&btn_capture);
    btn_cap_row.append(&btn_record);

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
        .tooltip_text("Espelha horizontalmente apenas o vídeo da câmera (não afeta a imagem de standby)")
        .build();
    let lbl_mirror_row = gtk4::Label::builder()
        .label("Espelhar vídeo da câmera")
        .tooltip_text("Espelha horizontalmente apenas o vídeo da câmera (não afeta a imagem de standby)")
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
    left.append(&cam_box);
    left.append(&btn_cam_row);
    left.append(&btn_cap_row);
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

    // captura de foto
    let cmd_tx = engine.borrow().cmd_tx.clone();
    btn_capture.connect_clicked(move |_| {
        let _ = cmd_tx.send(EngineCmd::Capture);
    });

    // gravação de vídeo
    let is_recording = Rc::new(std::cell::Cell::new(false));
    let is_recording_btn = is_recording.clone();
    let cmd_tx_rec = engine.borrow().cmd_tx.clone();
    btn_record.connect_clicked(move |_| {
        if is_recording_btn.get() {
            let _ = cmd_tx_rec.send(EngineCmd::StopRecording);
        } else {
            let _ = cmd_tx_rec.send(EngineCmd::StartRecording);
        }
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

    // abertura e controle da janela de configurações
    let audio_sources_data = Rc::new(RefCell::new(Vec::<String>::new()));
    let audio_sources_combo = audio_sources_data.clone();
    let audio_combo_setup = audio_combo.clone();

    let open_settings = {
        let settings_win = settings_win.clone();
        let cmd_tx = engine.borrow().cmd_tx.clone();
        let lbl_scan_status = lbl_scan_status.clone();
        let suppress = suppress.clone();
        let ctrl_grid_open = ctrl_grid.clone();
        let ctrl_checks_open = ctrl_checks.clone();
        let ctrl_syncing_open = ctrl_syncing.clone();
        let visible_state_sync = visible_state.clone();
        let last_tree_sync = last_tree.clone();
        let cam_on_checks = cam_on.clone();
        Rc::new(move || {
            lbl_scan_status.set_text("Buscando câmeras…");
            let _ = cmd_tx.send(EngineCmd::SetScanning(true));
            let _ = cmd_tx.send(EngineCmd::ScanCameras);

            // reconstrói os checkboxes: catálogo curado + todos os controles
            // que a câmera reproduz na última árvore lida
            let tree = last_tree_sync.borrow().clone();
            rebuild_ctrl_checks(
                &ctrl_grid_open,
                &ctrl_checks_open,
                &ctrl_syncing_open,
                &visible_state_sync,
                &tree,
                &cmd_tx,
                &cam_on_checks,
            );

            // Atualiza lista de dispositivos de áudio
            #[allow(deprecated)]
            let current_active = audio_combo_setup.active();
            let sources = crate::core::audio::list_audio_sources();
            suppress.set(true);
            #[allow(deprecated)]
            audio_combo_setup.remove_all();
            #[allow(deprecated)]
            audio_combo_setup.append_text("Padrão do Sistema (default)");
            let mut ids = vec!["default".to_string()];
            for src in &sources {
                #[allow(deprecated)]
                audio_combo_setup.append_text(&src.description);
                ids.push(src.id.clone());
            }
            #[allow(deprecated)]
            audio_combo_setup.append_text("Sem áudio (mudo)");
            ids.push("none".to_string());
            *audio_sources_combo.borrow_mut() = ids;

            #[allow(deprecated)]
            if let Some(act) = current_active {
                audio_combo_setup.set_active(Some(act));
            } else {
                audio_combo_setup.set_active(Some(0));
            }
            suppress.set(false);

            settings_win.present();
        })
    };

    let open_settings_hdr = open_settings.clone();
    btn_settings.connect_clicked(move |_| {
        open_settings_hdr();
    });

    let open_settings_cam = open_settings.clone();
    btn_settings_cam.connect_clicked(move |_| {
        open_settings_cam();
    });

    let cmd_tx_ref = engine.borrow().cmd_tx.clone();
    let lbl_scan_status_ref = lbl_scan_status.clone();
    btn_refresh_cams.connect_clicked(move |_| {
        lbl_scan_status_ref.set_text("Atualizando lista de câmeras…");
        let _ = cmd_tx_ref.send(EngineCmd::ScanCameras);
    });

    let settings_win_cls = settings_win.clone();
    let cmd_tx_cls = engine.borrow().cmd_tx.clone();
    btn_close_settings.connect_clicked(move |_| {
        let _ = cmd_tx_cls.send(EngineCmd::SetScanning(false));
        settings_win_cls.set_visible(false);
    });

    let settings_win_req = settings_win.clone();
    let cmd_tx_req = engine.borrow().cmd_tx.clone();
    settings_win.connect_close_request(move |_| {
        let _ = cmd_tx_req.send(EngineCmd::SetScanning(false));
        settings_win_req.set_visible(false);
        glib::Propagation::Stop
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

    // seleção de fonte de áudio
    let cmd_tx_audio = engine.borrow().cmd_tx.clone();
    let suppress_audio = suppress.clone();
    let audio_sources_listener = audio_sources_data.clone();
    #[allow(deprecated)]
    audio_combo.connect_changed(move |cb| {
        if suppress_audio.get() {
            return;
        }
        if let Some(ix) = cb.active()
            && let Some(src_id) = audio_sources_listener.borrow().get(ix as usize)
        {
            let _ = cmd_tx_audio.send(EngineCmd::SetAudioSource(src_id.clone()));
        }
    });

    // seleção de formato de vídeo
    let cmd_tx_vfmt = engine.borrow().cmd_tx.clone();
    let suppress_vfmt = suppress.clone();
    #[allow(deprecated)]
    vfmt_combo.connect_changed(move |cb| {
        if suppress_vfmt.get() {
            return;
        }
        let fmt = match cb.active() {
            Some(1) => "mkv",
            Some(2) => "webm",
            Some(3) => "mov",
            Some(4) => "avi",
            _ => "mp4",
        };
        let _ = cmd_tx_vfmt.send(EngineCmd::SetVideoFormat(fmt.to_string()));
    });

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
    let picture = gtk4::Picture::builder().content_fit(gtk4::ContentFit::ScaleDown).build();    let lbl_fps = gtk4::Label::builder()
        .label("fps: –")
        .halign(Align::End)
        .css_classes(["dim-label"])
        .build();
    let frame = gtk4::Frame::new(None);
    frame.set_child(Some(&picture));
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    frame.set_size_request(400, 300);

    // menu de contexto do preview (clique direito): captura, live view, pasta
    let frame_gc = gtk4::GestureClick::new();
    frame_gc.set_button(3);
    let cmd_tx_ctx = engine.borrow().cmd_tx.clone();
    let live_on_ctx = live_on.clone();
    let cam_on_ctx = cam_on.clone();
    let frame_for_menu = frame.clone();
    frame_gc.connect_pressed(move |g, _n, x, y| {
        g.set_state(gtk4::EventSequenceState::Claimed);
        let mut items: Vec<(String, Box<dyn FnOnce()>)> = Vec::new();
        if cam_on_ctx.get() {
            items.push((
                "Capturar foto (full-res)".into(),
                {
                    let cmd_tx = cmd_tx_ctx.clone();
                    Box::new(move || { let _ = cmd_tx.send(EngineCmd::Capture); })
                },
            ));
            if live_on_ctx.get() {
                items.push((
                    "Parar Live View".into(),
                    {
                        let cmd_tx = cmd_tx_ctx.clone();
                        Box::new(move || { let _ = cmd_tx.send(EngineCmd::SetLiveView(false)); })
                    },
                ));
            } else {
                items.push((
                    "Iniciar Live View".into(),
                    {
                        let cmd_tx = cmd_tx_ctx.clone();
                        Box::new(move || { let _ = cmd_tx.send(EngineCmd::SetLiveView(true)); })
                    },
                ));
            }
        }
        items.push((
            "Abrir pasta da sessão".into(),
            Box::new(move || {
                let dir = camera::session_dir();
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            }),
        ));
        show_context_menu(frame_for_menu.upcast_ref(), x, y, items);
    });
    frame.add_controller(frame_gc);

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
    right_frame.set_size_request(340, -1);
    right_frame.set_hexpand(false);

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
        btn_record,
        cam_combo,
        audio_combo,
        vfmt_combo,
        lbl_scan_status,
        btn_liveview,
        dev_combo,
        lbl_session,
        gal_box,
        config_box,
        config_scroll,
        window: window.clone(),
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
    poll_events(app, &window, engine, action_rx, w, cam_on, live_on, is_recording, cameras_data, suppress, visible_state, last_tree);
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

/// Normaliza um identificador: minúsculas, só alfanuméricos
/// ("ISO Speed Rate" → "isospeedrate", "/camera/shutterspeed2" → "shutterspeed2")
fn normalize_id(s: &str) -> String {
    s.to_ascii_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Filtra a árvore de configuração da câmera: apenas os controles marcados
/// como visíveis (tela de configurações) são renderizados. Seções que ficarem
/// vazias são removidas junto. matching por nome/label (aliases, sem acento).
fn filter_visible_controls(nodes: &[CfgNode], visible: &[String]) -> Vec<CfgNode> {
    nodes
        .iter()
        .filter_map(|n| match n.kind {
            CfgKind::Section => {
                let children = filter_visible_controls(&n.children, visible);
                if children.is_empty() {
                    None
                } else {
                    Some(CfgNode { children, ..n.clone() })
                }
            }
            _ => leaf_visible(n, visible).then(|| n.clone()),
        })
        .collect()
}

/// true = o widget corresponde a um controle marcado como visível.
/// A lista visível pode conter chaves do catálogo curado (matched por aliases
/// de nome/label) ou chaves dinâmicas (nome normalizado do widget da câmera).
fn leaf_visible(n: &CfgNode, visible: &[String]) -> bool {
    if n.readonly {
        return false; // controles só de leitura nunca aparecem (info, não controle)
    }
    let short = n.name.rsplit('/').next().unwrap_or(&n.name);
    let name_n = normalize_id(short);
    let label_n = normalize_id(&n.label);
    // chave direta na lista visível (chaves dinâmicas da câmera)
    if visible.iter().any(|k| {
        let k_n = normalize_id(k);
        k_n == name_n || (!label_n.is_empty() && k_n == label_n)
    }) {
        return true;
    }
    // aliases do catálogo curado
    CONTROLS_CATALOG.iter().any(|(key, _, aliases)| {
        visible.iter().any(|k| k == key)
            && aliases
                .iter()
                .any(|a| normalize_id(a) == name_n || normalize_id(a) == label_n)
    })
}

/// Coleta folhas alteráveis da árvore (chave = último segmento normalizado).
fn collect_leaf_controls(nodes: &[CfgNode], out: &mut Vec<(String, String)>) {
    for n in nodes {
        if matches!(n.kind, CfgKind::Section) {
            collect_leaf_controls(&n.children, out);
            continue;
        }
        if matches!(n.kind, CfgKind::Date) || n.readonly {
            continue;
        }
        let short = n.name.rsplit('/').next().unwrap_or(&n.name);
        let key = normalize_id(short);
        if key.is_empty() {
            continue;
        }
        let label = if n.label.is_empty() { short.to_string() } else { n.label.clone() };
        out.push((key, label));
    }
}

/// Monta a lista de entradas dos checkboxes: catálogo curado (mais usados
/// primeiro) + todos os demais controles que a câmera expõe na última árvore.
/// `true` = vem do catálogo.
fn collect_control_entries(tree: &[CfgNode]) -> Vec<(String, String, bool)> {
    let mut entries: Vec<(String, String, bool)> = CONTROLS_CATALOG
        .iter()
        .map(|(k, l, _)| ((*k).to_string(), (*l).to_string(), true))
        .collect();
    let catalog_aliases: Vec<String> = CONTROLS_CATALOG
        .iter()
        .flat_map(|(_, _, a)| a.iter().map(|x| normalize_id(x)).collect::<Vec<_>>())
        .collect();

    let mut leaves: Vec<(String, String)> = Vec::new();
    collect_leaf_controls(tree, &mut leaves);
    for (key, label) in leaves {
        let in_catalog = CONTROLS_CATALOG
            .iter()
            .any(|(k, _aliases, _)| normalize_id(k) == key || catalog_aliases.contains(&key));
        if !in_catalog {
            match entries.iter_mut().find(|(k, _, _)| *k == key) {
                Some(e) => e.1 = label, // catálogo com nome real da câmera
                None => entries.push((key, label, false)),
            }
        }
    }
    entries
}

/// Reconstrói os checkboxes de "Controles da Câmera": catálogo curado (mais
/// usados primeiro) + todos os demais controles que a câmera realmente expõe
/// na última árvore lida. Marca os visíveis e envia SetVisibleControls a cada
/// toggle (com re-render do painel quando a câmera está conectada).
#[allow(clippy::too_many_arguments)]
fn rebuild_ctrl_checks(
    grid: &gtk4::Grid,
    checks: &Rc<RefCell<Vec<(String, gtk4::CheckButton)>>>,
    syncing: &Rc<std::cell::Cell<bool>>,
    visible_state: &Rc<RefCell<Vec<String>>>,
    tree: &[CfgNode],
    cmd: &Sender<EngineCmd>,
    cam_on: &Rc<std::cell::Cell<bool>>,
) {
    while let Some(c) = grid.first_child() {
        grid.remove(&c);
    }

    let entries = collect_control_entries(tree);

    // cria os checkboxes (grid de 2 colunas)
    let mut new_checks = Vec::new();
    for (i, (key, label, from_catalog)) in entries.iter().enumerate() {
        let tip = if *from_catalog {
            let aliases = CONTROLS_CATALOG
                .iter()
                .find(|(k, _, _)| *k == key)
                .map(|(_, _, a)| a.join(", "))
                .unwrap_or_default();
            format!("Aparece no painel quando a câmera expõe: {aliases}")
        } else {
            "Controle exposto pela câmera conectada".to_string()
        };
        let cb = gtk4::CheckButton::builder()
            .label(label)
            .tooltip_text(tip)
            .build();
        grid.attach(&cb, (i % 2) as i32, (i / 2) as i32, 1, 1);
        let checks_h = checks.clone();
        let syncing_h = syncing.clone();
        let vs_h = visible_state.clone();
        let cmd_h = cmd.clone();
        let cam_h = cam_on.clone();
        cb.connect_toggled(move |_| {
            if syncing_h.get() {
                return;
            }
            let vis: Vec<String> = checks_h
                .borrow()
                .iter()
                .filter(|(_, c)| c.is_active())
                .map(|(k, _)| k.clone())
                .collect();
            *vs_h.borrow_mut() = vis.clone();
            let _ = cmd_h.send(EngineCmd::SetVisibleControls(vis));
            if cam_h.get() {
                let _ = cmd_h.send(EngineCmd::GetConfig);
            }
        });
        new_checks.push((key.clone(), cb));
    }
    *checks.borrow_mut() = new_checks;

    // marca os visíveis em silêncio (syncing impede eco dos handlers)
    syncing.set(true);
    let vis = visible_state.borrow().clone();
    for (key, cb) in checks.borrow().iter() {
        cb.set_active(vis.iter().any(|k| k == key));
    }
    syncing.set(false);
}

/// Menu de contexto (popover): itens = (rótulo, ação). Mostra em x/y relativos
/// ao widget e se remove depois.
fn show_context_menu(
    widget: &gtk4::Widget,
    x: f64,
    y: f64,
    items: Vec<(String, Box<dyn FnOnce()>)>,
) {
    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);
    pop.set_autohide(true);
    let box_ = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(2)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();
    for (label, act) in items {
        let b = gtk4::Button::builder().label(label).css_classes(["flat", "context-menu-item"]).build();
        let pop = pop.clone();
        let act = Rc::new(RefCell::new(Some(act)));
        b.connect_clicked(move |_| {
            pop.popdown();
            if let Some(a) = act.borrow_mut().take() {
                a();
            }
        });
        box_.append(&b);
    }
    pop.set_child(Some(&box_));
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x.round() as i32, y.round() as i32, 1, 1)));
    pop.set_parent(widget);
    // remove da hierarquia quando fechar — no próximo idle, para não mexer na
    // árvore durante o processamento do unmap (unparent em unrealize recursaria)
    let pop_idle = pop.clone();
    pop.connect_closed(move |_| {
        let pc = pop_idle.clone();
        glib::idle_add_local_once(move || {
            if pc.parent().is_some() {
                pc.unparent();
            }
        });
    });
    pop.popup();
}

/// Diálogo de confirmação de exclusão de mídia (galeria)
#[allow(deprecated)]
fn confirm_delete_media(
    parent: &gtk4::Window,
    path: std::path::PathBuf,
    after_delete: Box<dyn FnOnce()>,
) {
    let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dlg = MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .buttons(ButtonsType::YesNo)
        .text("Excluir mídia?")
        .secondary_text(format!("Excluir “{file_name}” do disco permanentemente?\n\nNão é possível desfazer."))
        .build();
    let act = Rc::new(RefCell::new(Some(after_delete)));
    let act_resp = act.clone();
    dlg.connect_response(move |d, resp| {
        if resp == gtk4::ResponseType::Yes
            && let Some(f) = act_resp.borrow_mut().take()
        {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    crate::core::log::log("INFO", &format!("mídia excluída: {}", path.display()));
                    f();
                }
                Err(e) => {
                    crate::core::log::log("ERROR", &format!("excluir {}: {e}", path.display()));
                    f(); // atualiza a galeria mesmo assim (estado real do disco)
                }
            }
        }
        d.destroy();
    });
    dlg.show();
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
    refresh_gallery_box(&w.gal_box, w.window.upcast_ref());
}

/// Galeria da sessão: thumbnails + clique abre o arquivo; clique direito abre
/// menu de contexto (Abrir / Abrir pasta / Excluir mídia do disco).
fn refresh_gallery_box(gal_box: &gtk4::Box, parent: &gtk4::Window) {
    while let Some(c) = gal_box.first_child() {
        gal_box.remove(&c);
    }
    let dir = camera::session_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|x| x.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
                Some("jpg" | "jpeg" | "cr2" | "cr3" | "png" | "mp4" | "mkv" | "avi" | "mov" | "webm")
            )
        })
        .filter_map(|p| {
            p.metadata().ok().map(|m| (p, m.modified().ok().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect();
    files.sort_by_key(|fa| std::cmp::Reverse(fa.1));
    if files.is_empty() {
        let l = gtk4::Label::builder().label("(sem fotos ou vídeos)".to_string()).build();
        l.set_sensitive(false);
        gal_box.append(&l);
    }
    for (p, _) in files.iter().take(24) {
        let btn = gtk4::Button::new();
        let img = gtk4::Image::builder().pixel_size(96).build();
        let ext = p.extension().and_then(|x| x.to_str()).map(|s| s.to_ascii_lowercase());
        let is_video = matches!(ext.as_deref(), Some("mp4" | "mkv" | "avi" | "mov" | "webm"));
        if matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png")) {
            img.set_from_file(Some(p));
        } else if is_video {
            img.set_icon_name(Some("video-x-generic"));
        } else {
            // RAW (CR2/CR3): GTK não decodifica — ícone genérico
            img.set_icon_name(Some("image-x-generic"));
        }
        btn.set_child(Some(&img));
        let size = p.metadata().map(|m| m.len()).unwrap_or(0);
        let tag = if is_video { " [Vídeo]" } else { "" };
        btn.set_tooltip_text(Some(&format!(
            "{}{} ({:.1} MB) — clique abre · clique direito exclui",
            p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            tag,
            size as f64 / 1e6
        )));

        // clique esquerdo: abrir o arquivo
        let path = p.clone();
        btn.connect_clicked(move |_| {
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        });

        // clique direito: menu de contexto (abrir / abrir pasta / excluir)
        let gc = gtk4::GestureClick::new();
        gc.set_button(3); // botão direito
        let path = p.clone();
        let gal_box_c = gal_box.clone();
        let parent_c = parent.clone();
        let btn_c = btn.clone();
        gc.connect_pressed(move |g, _n, x, y| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            let dir_path = path.parent().map(|d| d.to_path_buf()).unwrap_or_else(camera::session_dir);
            let items: Vec<(String, Box<dyn FnOnce()>)> = vec![
                (
                    "Abrir arquivo".into(),
                    {
                        let path = path.clone();
                        Box::new(move || {
                            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                        })
                    },
                ),
                (
                    "Abrir pasta".into(),
                    {
                        let dir_path = dir_path.clone();
                        Box::new(move || {
                            let _ = std::process::Command::new("xdg-open").arg(&dir_path).spawn();
                        })
                    },
                ),
                (
                    "Excluir mídia…".into(),
                    {
                        let path = path.clone();
                        let gal_box_c = gal_box_c.clone();
                        let parent_c = parent_c.clone();
                        Box::new(move || {
                            let parent_c = parent_c.clone();
                            let parent_dlg = parent_c.clone();
                            confirm_delete_media(&parent_dlg, path.clone(), Box::new(move || {
                                refresh_gallery_box(&gal_box_c, &parent_c);
                            }));
                        })
                    },
                ),
            ];
            show_context_menu(btn_c.upcast_ref(), x, y, items);
        });
        btn.add_controller(gc);
        gal_box.append(&btn);
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
    is_recording: Rc<std::cell::Cell<bool>>,
    cameras_data: Rc<RefCell<Vec<(String, String)>>>,
    suppress: Rc<std::cell::Cell<bool>>,
    visible_state: Rc<RefCell<Vec<String>>>,
    last_tree: Rc<RefCell<Vec<CfgNode>>>,
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
                        w.lbl_scan_status.set_text("Nenhuma câmera detectada via USB.");
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
                        w.lbl_scan_status.set_text(&format!("{} câmera(s) detectada(s).", cameras.len()));
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
                EngineEv::AppState { fps, webcam, mirror, zoom, pan_x, pan_y, out, autoconnect, video_device: _, liveview, audio_source, video_format, visible_controls } => {
                    suppress.set(true);
                    // lista salva como está: contém chaves do catálogo curado
                    // e/ou chaves dinâmicas (nomes reais da câmera)
                    *visible_state.borrow_mut() = if visible_controls.is_empty() {
                        settings::default_visible_controls()
                    } else {
                        visible_controls
                    };
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
                    #[allow(deprecated)]
                    w.vfmt_combo.set_active(match video_format.to_ascii_lowercase().as_str() {
                        "mkv" => Some(1),
                        "webm" => Some(2),
                        "mov" => Some(3),
                        "avi" => Some(4),
                        _ => Some(0),
                    });
                    if audio_source == "none" || audio_source == "disabled" {
                        #[allow(deprecated)]
                        w.audio_combo.set_active(Some(1));
                    } else {
                        #[allow(deprecated)]
                        w.audio_combo.set_active(Some(0));
                    }
                    live_on.set(liveview);
                    suppress.set(false);
                }
                EngineEv::Connected(on) => {
                    cam_on.set(on);
                    w.btn_connect.set_label(if on { "Desconectar câmera" } else { "Conectar câmera" });
                    w.btn_liveview.set_sensitive(on);
                    w.btn_capture.set_sensitive(on);
                    w.btn_record.set_sensitive(on);
                    if on {
                        w.lbl_status.set_text("câmera conectada (obturador em repouso — inicie o Live View para preview)");
                    } else {
                        live_on.set(false);
                        w.btn_liveview.set_label("Live View");
                        w.btn_liveview.remove_css_class("destructive-action");
                        w.btn_liveview.remove_css_class("suggested-action");
                        w.btn_record.set_label("Gravar vídeo");
                        w.btn_record.remove_css_class("destructive-action");
                        is_recording.set(false);
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
                EngineEv::ConfigTree(nodes_raw) => {
                    *last_tree.borrow_mut() = nodes_raw.clone();
                    let cmd = engine.borrow().cmd_tx.clone();
                    let adj = w.config_scroll.vadjustment();
                    let pos = adj.value();
                    if pos > 0.0 {
                        pending_scroll.set(Some(pos));
                    }
                    // filtro: apenas os controles marcados na tela de configurações
                    let nodes = filter_visible_controls(&nodes_raw, &visible_state.borrow());
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
                EngineEv::RecordingStarted { path } => {
                    is_recording.set(true);
                    w.btn_record.set_label("⏹ Parar (00:00)");
                    w.btn_record.add_css_class("destructive-action");
                    w.lbl_status.set_text(&format!("gravando vídeo: {path}"));
                }
                EngineEv::RecordingProgress { elapsed_secs, frames: _ } => {
                    let mins = elapsed_secs / 60;
                    let secs = elapsed_secs % 60;
                    w.btn_record.set_label(&format!("⏹ Parar ({mins:02}:{secs:02})"));
                    w.lbl_status.set_text(&format!("gravando vídeo… ({mins:02}:{secs:02})"));
                }
                EngineEv::RecordingDone { path, duration_secs, size_bytes, error } => {
                    is_recording.set(false);
                    w.btn_record.set_label("Gravar vídeo");
                    w.btn_record.remove_css_class("destructive-action");
                    match (path, error) {
                        (Some(p), _) => {
                            let mb = size_bytes as f64 / 1_000_000.0;
                            w.lbl_status.set_text(&format!("vídeo salvo ({duration_secs}s, {mb:.1} MB): {p}"));
                            refresh_gallery(&w);
                        }
                        (None, Some(e)) => {
                            w.lbl_status.set_markup(&format!("<b>erro gravação: {e}</b>"));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(name: &str, label: &str, readonly: bool) -> CfgNode {
        CfgNode {
            name: name.into(),
            label: label.into(),
            kind: CfgKind::Radio,
            readonly,
            info: String::new(),
            value: Some(CfgValue::Radio { options: vec![], current: String::new() }),
            children: vec![],
        }
    }

    fn section(name: &str, children: Vec<CfgNode>) -> CfgNode {
        CfgNode {
            name: name.into(),
            label: name.into(),
            kind: CfgKind::Section,
            readonly: false,
            info: String::new(),
            value: None,
            children,
        }
    }

    fn names(nodes: Vec<CfgNode>) -> Vec<String> {
        let mut v = Vec::new();
        for n in nodes {
            if n.kind == CfgKind::Section {
                v.extend(names(n.children));
            } else {
                v.push(n.name);
            }
        }
        v
    }

    #[test]
    fn default_keys_tempo_no_catalogo() {
        let catalog: Vec<&str> = CONTROLS_CATALOG.iter().map(|(k, _, _)| *k).collect();
        for k in settings::default_visible_controls() {
            assert!(catalog.contains(&k.as_str()), "chave '{k}' ausente do catálogo");
        }
    }

    #[test]
    fn controla_filtro_por_nome_e_label() {
        let vis = settings::default_visible_controls();
        let tree = [section(
            "camera",
            vec![
                leaf("/camera/iso", "ISO", false),
                leaf("/camera/shutterspeed2", "Shutter Speed 2", false),
                leaf("/f-number", "f-number", false),
                leaf("/camera/whitebalance", "White Balance", false),
                leaf("/camera/aemode", "AE mode", false),
                leaf("/camera/utilities/autofocusdrive", "Autofocus drive", false),
                leaf("/camera/ownername", "Owner name", true),
            ],
        )];
        let filtered = filter_visible_controls(&tree, &vis);
        let got = names(filtered);
        assert!(got.contains(&"/camera/iso".to_string()));
        assert!(got.contains(&"/camera/shutterspeed2".to_string()));
        assert!(got.contains(&"/f-number".to_string()));
        assert!(got.contains(&"/camera/whitebalance".to_string()));
        // não marcado no default:
        assert!(!got.contains(&"/camera/aemode".to_string()));
        assert!(!got.contains(&"/camera/utilities/autofocusdrive".to_string()));
        // readonly nunca aparece
        assert!(!got.contains(&"/camera/ownername".to_string()));
    }

    #[test]
    fn secao_vazia_e_removida() {
        let vis = settings::default_visible_controls();
        let tree = [
            section("camera", vec![leaf("/camera/iso", "ISO", false)]),
            section("actions", vec![leaf("/actions/autofocusdrive", "Autofocus drive", false)]),
            section("info", vec![leaf("/info/model", "Camera Model", true)]),
        ];
        let filtered = filter_visible_controls(&tree, &vis);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "camera");
    }

    #[test]
    fn lista_vazia_esconde_tudo() {
        let tree = [section("camera", vec![leaf("/camera/iso", "ISO", false)])];
        let filtered = filter_visible_controls(&tree, &[]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn chaves_dinâmicas_da_câmera_são_respeitadas() {
        // chave não-catalogada (nome real do widget) marcada direto na lista
        let vis = vec!["drivemode".to_string(), "reviewtime".to_string()];
        let tree = [section(
            "camera",
            vec![
                leaf("/camera/drivemode", "Drive Mode", false),
                leaf("/camera/reviewtime", "Review Time", false),
                leaf("/camera/beep", "Beep", false),
            ],
        )];
        let got = names(filter_visible_controls(&tree, &vis));
        assert!(got.contains(&"/camera/drivemode".to_string()));
        assert!(got.contains(&"/camera/reviewtime".to_string()));
        assert!(!got.contains(&"/camera/beep".to_string()));
    }

    #[test]
    fn coleta_folhas_da_árvore_para_tela_de_config() {
        let tree = [
            section("camera", vec![
                leaf("/camera/iso", "ISO", false),
                leaf("/camera/beep", "Beep", false),
                leaf("/info/model", "Camera Model", true),
                leaf("/actions/autofocusdrive", "Autofocus drive", false),
            ]),
        ];
        let mut out = Vec::new();
        collect_leaf_controls(&tree, &mut out);
        let keys: Vec<String> = out.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["iso", "beep", "autofocusdrive"]);
    }

    #[test]
    fn rebuild_não_duplica_chaves_do_catálogo() {
        // widget "iso" da árvore é coberto pelo catálogo — só os extras entram
        let tree = [
            section("camera", vec![
                leaf("/camera/iso", "ISO", false),
                leaf("/camera/beep", "Beep", false),
                leaf("/camera/drivemode", "Drive Mode", false),
            ]),
        ];
        let entries = collect_control_entries(&tree);
        let cat: Vec<&str> = entries.iter().filter(|e| e.2).map(|e| e.0.as_str()).collect();
        assert!(cat == CONTROLS_CATALOG.iter().map(|(k, _, _)| *k).collect::<Vec<_>>());
        let extras: Vec<&str> = entries.iter().filter(|e| !e.2).map(|e| e.0.as_str()).collect();
        assert_eq!(extras, vec!["beep", "drivemode"]);
    }
}

