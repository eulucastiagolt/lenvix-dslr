//! core-camera: thread dedicada à câmera (process_events do Entangle + loop webcamize).
//! A GUI não toca no handle do gphoto2 (arq. 4.1).
//!
//! Fase 3c:
//! • Câmera sob demanda (estilo Entangle): a engine inicia em standby e só
//!   reivindica o USB quando o usuário dá Connect — ou autoconnect via hotplug.
//! • Webcam virtual SEMPRE disponível: sem câmera, alimenta o loopback com
//!   frame standby ("EOS-like") no fps alvo — viewers nunca veem device morto.
//! • Saída fake: a resolução da câmera nunca muda; a webcam virtual recebe o
//!   crop-fill do aspecto escolhido, re-tentando a cada frame e reiniciando o
//!   writer sozinha se o mismatch se arrastar (troca ao vivo sem restart).
//! • settings.conf persiste fps/zoom/pan/mirror/saída/autoconnect.

use crate::core::{config::*, settings, stream, transform::{self, Transform}, yuv};
use crossbeam_channel::{bounded, Receiver, Sender};
use gphoto2::{list::CameraDescriptor, widget::Widget, Camera, Context};
use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Comandos GUI/tray → engine
#[derive(Debug)]
#[allow(dead_code)]
pub enum EngineCmd {
    SetWebcam(bool),
    SetFps(u32),
    Reselect { model: String, port: String },
    /// conecta/desconecta a câmera (preview só roda conectada — Entangle)
    SetConnected(bool),
    /// requisita snapshot atual da árvore de configuração
    GetConfig,
    /// aplica um valor (string; parse por tipo) e re-empurra a tree
    SetConfig { name: String, value: String },
    /// dispara botão do painel de config (foco, etc.)
    PressButton { name: String },
    /// captura full-res + download para a sessão local
    Capture,
    /// liga/desliga espelhamento horizontal (preview + webcam)
    SetMirror(bool),
    /// zoom (1.0–4.0) / pan (-100..100) / tamanho de saída fake (None = nativo)
    SetZoom(f32),
    SetZoomPos { x: Option<i16>, y: Option<i16> },
    SetOutSize(Option<(u32, u32)>),
    SetLiveView(bool),
    SetVideoDevice(u32),
    CreateNewVideoDevice,
    /// autoconectar quando a câmera aparecer no USB (hotplug, poll c/ backoff)
    SetAutostart(bool),
    Quit,
}

/// Eventos engine → GUI
#[derive(Debug)]
pub enum EngineEv {
    /// novo frame (RGBA8) + fps medido (janela 2s)
    Frame { w: u32, h: u32, rgba: Vec<u8>, measured_fps: f64 },
    CameraOpened { model: String },
    CamerasList { cameras: Vec<(String, String)>, active: (String, String) },
    PreviewLost(String),
    WebcamStatus { on: bool, path: Option<String>, card: Option<String>, error: Option<String> },
    DevicesList { devices: Vec<stream::LoopbackDevInfo>, active: u32 },
    LiveViewStatus(bool),
    /// árvore de configuração atual (filhos da janela raiz)
    ConfigTree(Vec<CfgNode>),
    /// captura em progresso
    CaptureBusy,
    /// captura+download concluída (err = None se ok) com o arquivo da sessão
    CaptureDone { path: Option<String>, error: Option<String> },
    /// estado persistido inicial (GUI sincroniza controles com settings.conf)
    AppState {
        fps: u32,
        webcam: bool,
        mirror: bool,
        zoom: f32,
        pan_x: i16,
        pan_y: i16,
        out: Option<(u32, u32)>,
        autoconnect: bool,
        video_device: u32,
        liveview: bool,
    },
    /// câmera conectada/desconectada (preview anda/para; standby assume)
    Connected(bool),
    Fatal(String),
}

pub const DEFAULT_VIDEO_DEVICE: u32 = 9; // evita /dev/video0 de webcams físicas
const HOTPLUG_MIN: Duration = Duration::from_millis(1500);
const HOTPLUG_MAX: Duration = Duration::from_millis(30_000);
/// erros seguidos de preview antes de desconectar sozinho (USB morto/deep-sleep)
const CAM_ERR_LIMIT: u32 = 5;

struct StandbyCache {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    eff: (u32, u32),
    yuyv: Vec<u8>,
}

/// card label do device virtual: "Lenvix (Canon EOS 4000D)"
fn card_label(model: &str) -> String {
    format!("Lenvix ({model})")
}

pub struct CameraEngine {
    pub cmd_tx: Sender<EngineCmd>,
    pub ev_rx: Receiver<EngineEv>,
    handle: Option<JoinHandle<()>>,
}

impl CameraEngine {
    /// Spawna a thread da câmera. Erros de setup chegam como `EngineEv::Fatal`.
    pub fn spawn() -> Self {
        let (ev_tx, ev_rx) = bounded::<EngineEv>(8);
        let (cmd_tx, cmd_rx) = bounded::<EngineCmd>(16);

        let handle = thread::Builder::new()
            .name("core-camera".into())
            .spawn(move || {
                if let Err(e) = run(&ev_tx, &cmd_rx) {
                    crate::core::log::log("ERROR", &format!("FATAL: {e}"));
                    let _ = ev_tx.send_timeout(EngineEv::Fatal(e), Duration::from_secs(2));
                }
            })
            .expect("spawn core-camera");

        Self { cmd_tx, ev_rx, handle: Some(handle) }
    }

    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(EngineCmd::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for CameraEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ===============================================================================

fn send_devices_list(
    ev_tx: &Sender<EngineEv>,
    active: u32,
    conflicted: &std::collections::HashSet<u32>,
) {
    let mut devs = stream::list_loopback_devices_detailed();
    for dev in &mut devs {
        if conflicted.contains(&dev.nr) {
            dev.in_use = true;
        }
    }
    send(ev_tx, EngineEv::DevicesList { devices: devs, active });
}

/// Resolve as dimensões da webcam virtual (/dev/videoN).
/// Tenta aplicar o formato `want` (16:9, ex.: 1920×1080).
/// Se o kernel recusar ou clampar (ex.: OBS ou navegador segurando buffers na resolução antiga):
/// 1. Marca o dispositivo atual como em conflito/indisponível.
/// 2. Cria/encontra automaticamente uma nova saída desocupada (/dev/video10 etc.).
/// 3. Migra o stream para a nova saída com a resolução total desejada (1920×1080).
/// 4. Notifica a UI e os observadores.
fn resolve_out(
    writer: &mut Option<stream::LoopbackWriter>,
    video_device: &mut u32,
    conflicted: &mut std::collections::HashSet<u32>,
    want: (u32, u32),
    last_fmt_warn: &mut Option<(u32, u32)>,
    ev_tx: &Sender<EngineEv>,
) -> (u32, u32) {
    let want = (want.0 & !1, want.1 & !1);
    let Some(wr) = writer.as_mut() else { return want };

    match wr.ensure_format(want.0, want.1) {
        Ok(()) => {
            if last_fmt_warn.take().is_some() {
                crate::core::log::log("INFO", &format!("saída webcam {}×{} aplicada com sucesso", want.0, want.1));
                send(ev_tx, EngineEv::WebcamStatus {
                    on: true,
                    path: Some(wr.path().into()),
                    card: Some(wr.card_name()),
                    error: None,
                });
            }
            want
        }
        Err(fmt_err) => {
            let old_nr = wr.device_nr();
            let card_label = wr.card_name();
            conflicted.insert(old_nr);
            crate::core::log::log(
                "WARN",
                &format!(
                    "{fmt_err}. Resolução recusada no /dev/video{old_nr} — criando nova saída virtual...",
                ),
            );

            // Migra automaticamente criando/abrindo nova saída limpa
            match stream::create_or_find_free_loopback(conflicted, &card_label) {
                Ok(mut new_writer) => {
                    let new_nr = new_writer.device_nr();
                    // Aplica o formato desejado no novo dispositivo desimpedido
                    match new_writer.ensure_format(want.0, want.1) {
                        Ok(()) => {
                            *video_device = new_nr;
                            *writer = Some(new_writer);
                            crate::core::log::log(
                                "INFO",
                                &format!(
                                    "migração concluída com sucesso: /dev/video{old_nr} (ocupado) → /dev/video{new_nr} ({}×{})",
                                    want.0, want.1
                                ),
                            );
                            send_devices_list(ev_tx, *video_device, conflicted);
                            send(ev_tx, EngineEv::WebcamStatus {
                                on: true,
                                path: Some(format!("/dev/video{new_nr}")),
                                card: Some(format!("{card_label} (/dev/video{new_nr})")),
                                error: Some(format!(
                                    "/dev/video{old_nr} ocupado pelo OBS — migrado para /dev/video{new_nr} ({}×{})",
                                    want.0, want.1
                                )),
                            });
                            *last_fmt_warn = None;
                            return want;
                        }
                        Err(e2) => {
                            crate::core::log::log("ERROR", &format!("formato no novo /dev/video{new_nr}: {e2}"));
                            *video_device = new_nr;
                            *writer = Some(new_writer);
                            send_devices_list(ev_tx, *video_device, conflicted);
                        }
                    }
                }
                Err(create_err) => {
                    crate::core::log::log("ERROR", &format!("criar nova saída virtual: {create_err}"));
                }
            }

            // Fallback de emergência caso criação falhe
            let (dw, dh) = if let Some(w) = writer.as_ref() { w.dims() } else { (0, 0) };
            let eff = if dw > 0 && dh > 0 { (dw, dh) } else { want };
            if last_fmt_warn.as_ref() != Some(&want) {
                let msg = format!(
                    "webcam em {}×{} (/dev/video{old_nr} ocupado por outro app — use o '+' para gerar nova saída)",
                    eff.0, eff.1
                );
                send(ev_tx, EngineEv::WebcamStatus {
                    on: true,
                    path: writer.as_ref().map(|w| w.path().into()),
                    card: writer.as_ref().map(|w| w.card_name()),
                    error: Some(msg),
                });
                *last_fmt_warn = Some(want);
            }
            eff
        }
    }
}

/// Fecha e reabre o writer da webcam (sem pkexec: usa o card label atual).
fn restart_writer(
    writer: &mut Option<stream::LoopbackWriter>,
    ev_tx: &Sender<EngineEv>,
    preferred_n: u32,
    why: &str,
) {
    let Some(cur) = writer.take() else { return };
    let card = cur.card_name();
    drop(cur);
    std::thread::sleep(Duration::from_millis(60));
    crate::core::log::log("INFO", &format!("webcam reiniciada ({why})"));
    match stream::ensure_loopback(preferred_n, &card) {
        Ok(w) => {
            send(ev_tx, EngineEv::WebcamStatus {
                on: true,
                path: Some(w.path().into()),
                card: Some(w.card_name()),
                error: None,
            });
            *writer = Some(w);
        }
        Err(e) => {
            crate::core::log::log("ERROR", &format!("reiniciar webcam falhou: {e}"));
            send(ev_tx, EngineEv::WebcamStatus {
                on: false,
                path: None,
                card: None,
                error: Some(format!("webcam interrompida ao reiniciar: {e}")),
            });
        }
    }
}

/// Escreve o frame YUYV. Retorno true = writer morto (evento já enviado).
fn push_yuyv(
    writer: &mut stream::LoopbackWriter,
    ev_tx: &Sender<EngineEv>,
    yuyv: &[u8],
) -> bool {
    if let Err(e) = writer.write_frame(yuyv) {
        let e = format!("webcam interrompida: {e}");
        crate::core::log::log("ERROR", &e);
        send(ev_tx, EngineEv::WebcamStatus {
            on: false,
            path: None,
            card: None,
            error: Some(e),
        });
        return true;
    }
    false
}

/// Lista câmeras disponíveis via libgphoto2
fn scan_cameras(context: &Context) -> Result<Vec<(String, String)>, String> {
    context
        .list_cameras()
        .wait()
        .map(|l| l.map(|d| (d.model, d.port)).collect())
        .map_err(|e| format!("list_cameras: {e}"))
}

/// Conecta à câmera indicada em `target` (ou 1.ª disponível se None).
fn connect_camera(
    context: &Context,
    ev_tx: &Sender<EngineEv>,
    target: Option<&(String, String)>,
) -> Result<Option<(Camera, String, String)>, String> {
    let list = scan_cameras(context)?;
    let chosen = if let Some(t) = target {
        list.iter().find(|(m, p)| m == &t.0 && p == &t.1).cloned()
    } else {
        list.first().cloned()
    };
    let Some((model, port)) = chosen.or_else(|| list.first().cloned()) else { return Ok(None) };
    let camera = open_desc(context, &model, &port)?;
    send(ev_tx, EngineEv::CamerasList {
        cameras: list,
        active: (model.clone(), port.clone()),
    });
    send(ev_tx, EngineEv::CameraOpened { model: model.clone() });
    send(ev_tx, EngineEv::Connected(true));
    send(ev_tx, EngineEv::LiveViewStatus(false));
    Ok(Some((camera, model, port)))
}

fn open_desc(context: &Context, model: &str, port: &str) -> Result<Camera, String> {
    let camera = context
        .get_camera(&CameraDescriptor {
            model: model.to_string(),
            port: port.to_string(),
        })
        .wait()
        .map_err(|e| format!("abrir {model}: {e}"))?;
    crate::core::log::log("INFO", &format!("câmera aberta: {model} [{port}]"));
    Ok(camera)
}

// ===============================================================================

#[allow(clippy::too_many_lines)]
fn run(ev_tx: &Sender<EngineEv>, cmd_rx: &Receiver<EngineCmd>) -> Result<(), String> {
    crate::core::log::log("INFO", "engine iniciada (standby — câmera sob demanda)");
    let context = Context::new().map_err(|e| format!("Context::new: {e}"))?;

    // estado persistido (settings.conf): aplica antes de qualquer conexão
    let saved = settings::load();
    let mut target_fps = saved.fps;
    let mut webcam_on = saved.webcam;
    let mut mirror = saved.mirror;
    let mut transf = Transform {
        zoom: saved.zoom,
        pan_x: saved.pan_x,
        pan_y: saved.pan_y,
        out: saved.out,
    };
    let mut autoconnect = saved.autoconnect;
    let mut video_device = saved.video_device;
    let mut liveview_on = false;

    let mut cam: Option<Camera> = None;
    let mut conn: Option<(String, String)> = None;
    let mut writer: Option<stream::LoopbackWriter> = None;
    let mut frame_times: Vec<Instant> = Vec::with_capacity(64);
    let mut last_fmt_warn: Option<(u32, u32)> = None; // debounce do aviso de troca
    let mut standby: Option<StandbyCache> = None;
    let mut last_hotplug = Instant::now() - HOTPLUG_MIN; // poll já no startup
    let mut hotplug_every = HOTPLUG_MIN;
    let mut cam_errs: u32 = 0;
    let mut selected_target: Option<(String, String)> = None;
    let mut last_detected_cameras: Option<Vec<(String, String)>> = None;

    // Inicializa a webcam virtual se estiver ligada por padrão
    if webcam_on {
        match stream::ensure_loopback(video_device, "Lenvix") {
            Ok(w) => {
                let card = w.card_name();
                crate::core::log::log(
                    "INFO",
                    &format!("webcam ligada por padrão: {} ({card})", w.path()),
                );
                send(ev_tx, EngineEv::WebcamStatus {
                    on: true,
                    path: Some(w.path().into()),
                    card: Some(card),
                    error: None,
                });
                writer = Some(w);
            }
            Err(e) => {
                crate::core::log::log("WARN", &format!("webcam inicial não disponível: {e}"));
                send(ev_tx, EngineEv::WebcamStatus {
                    on: false,
                    path: None,
                    card: None,
                    error: Some(e),
                });
            }
        }
    }

    // sincroniza a GUI com o estado salvo ANTES de qualquer interação
    send(ev_tx, EngineEv::AppState {
        fps: target_fps,
        webcam: webcam_on,
        mirror,
        zoom: transf.zoom,
        pan_x: transf.pan_x,
        pan_y: transf.pan_y,
        out: transf.out,
        autoconnect,
        video_device,
        liveview: liveview_on,
    });
    let mut conflicted_devices: std::collections::HashSet<u32> = std::collections::HashSet::new();
    send_devices_list(ev_tx, video_device, &conflicted_devices);
    send(ev_tx, EngineEv::Connected(false));
    send(ev_tx, EngineEv::LiveViewStatus(false));

    // persistência com throttle (sliders disparam dezenas de cmds por segundo)
    let snapshot = |fps: u32, web: bool, mir: bool, t: &Transform, ac: bool, vd: u32| settings::Settings {
        fps,
        webcam: web,
        mirror: mir,
        zoom: t.zoom,
        pan_x: t.pan_x,
        pan_y: t.pan_y,
        out: t.out,
        autoconnect: ac,
        video_device: vd,
    };
    let mut last_save = Instant::now();
    let mut save_st = |force: bool, s: &settings::Settings| {
        if force || last_save.elapsed() >= Duration::from_millis(300) {
            settings::save(s);
            last_save = Instant::now();
        }
    };

    loop {
        // —— comandos (GUI/tray) ————————————————————————————————————————
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                EngineCmd::Quit => {
                    settings::save(&snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device));
                    crate::core::log::log("INFO", "engine encerrada");
                    drop(writer.take()); // fecha fd e (limpeza FR-22)
                    drop(cam.take()); // libera o USB para outros apps
                    return Ok(());
                }
                EngineCmd::SetFps(f) => {
                    target_fps = f.max(1);
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                }
                EngineCmd::SetWebcam(on) => {
                    webcam_on = on;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                    last_fmt_warn = None;
                    match (&on, &mut writer) {
                        (true, None) => {
                            let label = conn
                                .as_ref()
                                .map(|(m, _)| card_label(m))
                                .unwrap_or_else(|| "Lenvix".into());
                            match stream::ensure_loopback(video_device, &label) {
                                Ok(w) => {
                                    let card = w.card_name();
                                    crate::core::log::log(
                                        "INFO",
                                        &format!(
                                            "webcam ligada: {} ({card}) — {}",
                                            w.path(),
                                            if cam.is_some() && liveview_on { "câmera ativa" } else { "standby" }
                                        ),
                                    );
                                    send(ev_tx, EngineEv::WebcamStatus {
                                        on: true,
                                        path: Some(w.path().into()),
                                        card: Some(card),
                                        error: None,
                                    });
                                    writer = Some(w);
                                }
                                Err(e) => {
                                    crate::core::log::log("ERROR", &format!("ligar webcam falhou: {e}"));
                                    send(ev_tx, EngineEv::WebcamStatus {
                                        on: false,
                                        path: None,
                                        card: None,
                                        error: Some(e),
                                    });
                                }
                            }
                        }
                        (false, Some(_)) => {
                            writer = None;
                            crate::core::log::log("INFO", "webcam desligada (fd fechado)");
                            send(ev_tx, EngineEv::WebcamStatus {
                                on: false,
                                path: None,
                                card: None,
                                error: None,
                            });
                        }
                        _ => {}
                    }
                }
                EngineCmd::SetLiveView(on) => {
                    if cam.is_some() {
                        liveview_on = on;
                        crate::core::log::log(
                            "INFO",
                            if on { "Live View iniciado (obturador aberto)" } else { "Live View pausado (obturador fechado)" }
                        );
                        send(ev_tx, EngineEv::LiveViewStatus(on));
                        if !on {
                            standby = None;
                        }
                    } else {
                        liveview_on = false;
                        send(ev_tx, EngineEv::LiveViewStatus(false));
                    }
                }
                EngineCmd::SetVideoDevice(dev_nr) => {
                    video_device = dev_nr;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                    crate::core::log::log("INFO", &format!("saída virtual alterada para /dev/video{dev_nr}"));
                    if writer.is_some() {
                        restart_writer(&mut writer, ev_tx, video_device, "troca de device virtual");
                    }
                    send_devices_list(ev_tx, video_device, &conflicted_devices);
                }
                EngineCmd::CreateNewVideoDevice => {
                    let card = writer.as_ref().map(|w| w.card_name()).unwrap_or_else(|| "Lenvix Camera".into());
                    match stream::create_or_find_free_loopback(&conflicted_devices, &card) {
                        Ok(new_wr) => {
                            let new_n = new_wr.device_nr();
                            video_device = new_n;
                            if writer.is_some() {
                                writer = Some(new_wr);
                            }
                            let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                            save_st(true, &s);
                            send_devices_list(ev_tx, video_device, &conflicted_devices);
                            crate::core::log::log("INFO", &format!("nova saída /dev/video{new_n} criada e selecionada"));
                        }
                        Err(e) => {
                            crate::core::log::log("ERROR", &format!("criar nova saída falhou: {e}"));
                        }
                    }
                }
                EngineCmd::SetConnected(do_connect) => {
                    if do_connect && cam.is_none() {
                        match connect_camera(&context, ev_tx, selected_target.as_ref()) {
                            Ok(Some((c, model, port))) => {
                                cam = Some(c);
                                conn = Some((model.clone(), port.clone()));
                                selected_target = Some((model, port));
                                cam_errs = 0;
                                liveview_on = false;
                                send(ev_tx, EngineEv::LiveViewStatus(false));
                                push_tree(cam.as_ref().unwrap(), ev_tx);
                            }
                            Ok(None) => send(ev_tx, EngineEv::PreviewLost(
                                "nenhuma câmera detectada — conecte via USB e clique em Conectar".into(),
                            )),
                            Err(e) => {
                                crate::core::log::log("WARN", &format!("conectar: {e}"));
                                send(ev_tx, EngineEv::PreviewLost(e));
                            }
                        }
                    } else if !do_connect && cam.take().is_some() {
                        conn = None;
                        liveview_on = false;
                        send(ev_tx, EngineEv::LiveViewStatus(false));
                        hotplug_every = HOTPLUG_MIN;
                        crate::core::log::log("INFO", "câmera desconectada (USB liberado)");
                        send(ev_tx, EngineEv::Connected(false));
                    }
                }
                EngineCmd::Reselect { model, port } => {
                    selected_target = Some((model.clone(), port.clone()));
                    if cam.is_some() {
                        drop(cam.take());
                        liveview_on = false;
                        send(ev_tx, EngineEv::LiveViewStatus(false));
                        match open_desc(&context, &model, &port) {
                            Ok(c) => {
                                conn = Some((model.clone(), port));
                                cam = Some(c);
                                cam_errs = 0;
                                send(ev_tx, EngineEv::CameraOpened { model });
                                send(ev_tx, EngineEv::Connected(true));
                                push_tree(cam.as_ref().unwrap(), ev_tx);
                            }
                            Err(e) => {
                                conn = None;
                                crate::core::log::log("WARN", &format!("reselect {model}: {e}"));
                                send(ev_tx, EngineEv::Connected(false));
                                send(ev_tx, EngineEv::PreviewLost(e));
                            }
                        }
                    }
                }
                EngineCmd::GetConfig => {
                    if let Some(c) = cam.as_ref() {
                        push_tree(c, ev_tx);
                    } else {
                        crate::core::log::log("WARN", "config pedida sem câmera");
                        send(ev_tx, EngineEv::PreviewLost("câmera não conectada".into()));
                    }
                }
                EngineCmd::SetConfig { name, value } => {
                    let Some(c) = cam.as_ref() else {
                        send(ev_tx, EngineEv::CaptureDone {
                            path: None,
                            error: Some("config: câmera não conectada".into()),
                        });
                        continue;
                    };
                    // Canon rejeita set_config logo após um preview ("I/O in
                    // progress"); retries crescentes até 4 (set_config NÃO
                    // dispara obturador — retry inofensivo)
                    let mut ok = false;
                    let mut last = String::new();
                    for attempt in 0..4 {
                        if attempt > 0 {
                            pause_busy_ms(600, attempt);
                        }
                        match apply_config(c, &name, &value) {
                            Ok(()) => {
                                ok = true;
                                last.clear();
                                break;
                            }
                            Err(e) => {
                                last = format!("config {name}: {e}");
                                crate::core::log::log("WARN", &last);
                                if !busy_like(&e) {
                                    break;
                                }
                            }
                        }
                    }
                    if ok {
                        crate::core::log::log("INFO", &format!("config {name}={value:?} aplicada"));
                        push_tree(c, ev_tx);
                    } else {
                        crate::core::log::log("ERROR", &format!("config {name}={value:?} falhou: {last}"));
                        send(ev_tx, EngineEv::CaptureDone { path: None, error: Some(last) });
                    }
                }
                EngineCmd::PressButton { name } => {
                    if let Some(c) = cam.as_ref() {
                        if let Err(e) = apply_config(c, &name, "__press__") {
                            send(ev_tx, EngineEv::CaptureDone {
                                path: None,
                                error: Some(format!("botão {name}: {e}")),
                            });
                        }
                    } else {
                        send(ev_tx, EngineEv::CaptureDone {
                            path: None,
                            error: Some(format!("botão {name}: câmera não conectada")),
                        });
                    }
                }
                EngineCmd::Capture => {
                    if let Some(c) = cam.as_ref() {
                        capture_photo(c, ev_tx);
                    } else {
                        send(ev_tx, EngineEv::CaptureDone {
                            path: None,
                            error: Some("captura: câmera não conectada".into()),
                        });
                    }
                }
                EngineCmd::SetMirror(v) => {
                    mirror = v;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                }
                EngineCmd::SetZoom(z) => {
                    transf.zoom = z;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(false, &s);
                }
                EngineCmd::SetZoomPos { x, y } => {
                    if let Some(x) = x {
                        transf.pan_x = x;
                    }
                    if let Some(y) = y {
                        transf.pan_y = y;
                    }
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(false, &s);
                }
                EngineCmd::SetOutSize(out) => {
                    transf.out = out;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                    standby = None; // limpa cache de frame standby para regenerar
                    last_fmt_warn = None;
                    crate::core::log::log(
                        "INFO",
                        &format!("saída webcam (fake): {}", match out {
                            Some((w, h)) => format!("{w}×{h}"),
                            None => "nativa".into(),
                        }),
                    );
                    // Reinicia a webcam virtual sozinho para aplicar o novo formato imediatamente
                    if writer.is_some() {
                        restart_writer(&mut writer, ev_tx, video_device, "troca de resolução fake");
                    }
                }
                EngineCmd::SetAutostart(v) => {
                    autoconnect = v;
                    let s = snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device);
                    save_st(true, &s);
                    crate::core::log::log("INFO", &format!("auto-conectar USB: {v}"));
                }
            }
        }

        // —— standby: sem câmera OU com câmera conectada mas Live View desligado ————
        if cam.is_none() || !liveview_on {
            if cam.is_none() && last_hotplug.elapsed() >= hotplug_every {
                last_hotplug = Instant::now();
                match scan_cameras(&context) {
                    Ok(list) => {
                        let changed = last_detected_cameras.as_ref() != Some(&list);
                        if changed {
                            last_detected_cameras = Some(list.clone());
                            let active = selected_target
                                .clone()
                                .filter(|t| list.iter().any(|(m, p)| m == &t.0 && p == &t.1))
                                .or_else(|| list.first().cloned())
                                .unwrap_or_default();
                            selected_target = if active.0.is_empty() { None } else { Some(active.clone()) };
                            send(ev_tx, EngineEv::CamerasList {
                                cameras: list.clone(),
                                active,
                            });
                        }
                        if autoconnect && !list.is_empty() {
                            match connect_camera(&context, ev_tx, selected_target.as_ref()) {
                                Ok(Some((c, model, port))) => {
                                    hotplug_every = HOTPLUG_MIN;
                                    cam = Some(c);
                                    conn = Some((model, port));
                                    cam_errs = 0;
                                    liveview_on = false;
                                    send(ev_tx, EngineEv::LiveViewStatus(false));
                                    push_tree(cam.as_ref().unwrap(), ev_tx);
                                    continue;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    crate::core::log::log("DEBUG", &format!("autoconnect: {e}"));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::core::log::log("DEBUG", &format!("scan_cameras: {e}"));
                    }
                }
            }

            let t0 = Instant::now();
            // standby: dims fake escolhidas > fallback 1080p (16:9 sempre!)
            let want = transf.out.unwrap_or((1920, 1080));
            let want = (want.0 & !1, want.1 & !1);
            let eff = if writer.is_some() {
                resolve_out(
                    &mut writer,
                    &mut video_device,
                    &mut conflicted_devices,
                    want,
                    &mut last_fmt_warn,
                    ev_tx,
                )
            } else {
                want
            };

            // gere/cache o frame standby (YUYV e RGBA)
            if standby.as_ref().is_none_or(|c| (c.w, c.h) != want || c.eff != eff) {
                let rgb = yuv::pattern_rgb(want.0, want.1);
                let mut rgba = Vec::with_capacity((want.0 * want.1 * 4) as usize);
                yuv::rgb_to_rgba(&rgb, &mut rgba);
                let yuyv = if eff == want {
                    yuv::rgb_to_yuyv(&rgb, want.0, want.1)
                } else {
                    let (_, _, eff_rgb) = transform::apply(
                        &rgb,
                        want.0,
                        want.1,
                        &Transform { zoom: 1.0, pan_x: 0, pan_y: 0, out: Some(eff) },
                    );
                    yuv::rgb_to_yuyv(&eff_rgb, eff.0, eff.1)
                };
                standby = Some(StandbyCache {
                    w: want.0,
                    h: want.1,
                    rgba,
                    eff,
                    yuyv,
                });
            }

            let cache = standby.as_ref().unwrap();

            // 1. Alimenta o loopback (se webcam ligada)
            let dead = if let Some(wr) = writer.as_mut() {
                push_yuyv(wr, ev_tx, &cache.yuyv)
            } else {
                false
            };

            // 2. Alimenta a GUI para que o preview se mantenha ativo mesmo sem câmera em 16:9!
            send(ev_tx, EngineEv::Frame {
                w: cache.w,
                h: cache.h,
                rgba: cache.rgba.clone(),
                measured_fps: f64::from(target_fps),
            });

            if dead {
                writer = None;
                standby = None;
            }

            // 3. Rate limiting monotônico (FR-06)
            let min = Duration::from_secs_f64(1.0 / f64::from(target_fps));
            if t0.elapsed() < min {
                std::thread::sleep(min - t0.elapsed());
            }
            continue;
        }

        // —— câmera conectada: pipeline de preview —————————————————————
        let c = cam.as_ref().expect("cam presente aqui");
        let t0 = Instant::now();
        let jpeg = match c.capture_preview().wait().and_then(|f| f.get_data(&context).wait()) {
            Ok(d) => d,
            Err(e) => {
                cam_errs += 1;
                crate::core::log::log(
                    "WARN",
                    &format!("capture_preview ({cam_errs}/{CAM_ERR_LIMIT}): {e}"),
                );
                send(ev_tx, EngineEv::PreviewLost(format!("capture_preview: {e}")));
                if cam_errs >= CAM_ERR_LIMIT {
                    // USB morto/deep-sleep: desconecta sozinho e volta ao standby
                    drop(cam.take());
                    conn = None;
                    last_fmt_warn = None;
                    hotplug_every = (hotplug_every * 2).min(HOTPLUG_MAX);
                    last_hotplug = Instant::now();
                    crate::core::log::log(
                        "WARN",
                        &format!("câmera desligada sozinha ({cam_errs} erros seguidos)"),
                    );
                    liveview_on = false;
                    send(ev_tx, EngineEv::LiveViewStatus(false));
                    send(ev_tx, EngineEv::Connected(false));
                } else {
                    std::thread::sleep(Duration::from_secs(1));
                }
                continue;
            }
        };
        cam_errs = 0;

        // 3. decode MJPEG → RGB (zune-jpeg, puro Rust)
        match decode_rgb(&jpeg) {
            Some((w0, h0, mut rgb)) => {
                if mirror {
                    yuv::flip_rgb_h(&mut rgb, w0, h0);
                }
                // zoom/pan/saída fake: crop-fill no aspecto da saída (GUI e master respeitam transf)
                let (w, h, rgb) = transform::apply(&rgb, w0, h0, &transf);
                let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                yuv::rgb_to_rgba(&rgb, &mut rgba);

                // 4. webcam: RGB → YUYV, write no loopback (FR-20)
                if writer.is_some() {
                    let eff = resolve_out(
                        &mut writer,
                        &mut video_device,
                        &mut conflicted_devices,
                        (w, h),
                        &mut last_fmt_warn,
                        ev_tx,
                    );
                    if let Some(wr) = writer.as_mut() {
                        let yuyv = if eff == (w, h) {
                            yuv::rgb_to_yuyv(&rgb, w, h)
                        } else {
                            let (_, _, eff_rgb) = transform::apply(
                                &rgb,
                                w,
                                h,
                                &Transform { zoom: 1.0, pan_x: 0, pan_y: 0, out: Some(eff) },
                            );
                            yuv::rgb_to_yuyv(&eff_rgb, eff.0, eff.1)
                        };
                        if push_yuyv(wr, ev_tx, &yuyv) {
                            writer = None;
                        }
                    }
                }

                // 5. fps real (janela móvel de 2s)
                let now = Instant::now();
                frame_times.push(now);
                frame_times.retain(|t| now.duration_since(*t).as_secs_f64() <= 2.0);
                let measured_fps = match frame_times.len() {
                    0 | 1 => 0.0,
                    n => {
                        let span = frame_times
                            .first()
                            .map(|t| now.duration_since(*t).as_secs_f64())
                            .unwrap_or(1.0)
                            .max(0.001);
                        (n as f64 - 1.0) / span
                    }
                };

                // 6. frame → GUI (não bloqueia: drop se a GUI estiver lenta)
                if ev_tx
                    .send_timeout(
                        EngineEv::Frame { w, h, rgba, measured_fps },
                        Duration::from_millis(250),
                    )
                    .is_err()
                {
                    settings::save(&snapshot(target_fps, webcam_on, mirror, &transf, autoconnect, video_device));
                    return Ok(()); // GUI saiu; fim
                }
            }
            None => {
                // falha de decode: fallback fiel webcamize (FR-24) — JPEG cru no loopback
                send(ev_tx, EngineEv::PreviewLost("decode MJPEG falhou".into()));
                if let Some(writer) = writer.as_mut() {
                    let _ = writer.write_frame(&jpeg);
                }
            }
        }

        // 7. rate-limit monotônico (FPS é teto, não garantia — §6.5)
        let elapsed = t0.elapsed();
        let min_period = Duration::from_secs_f64(1.0 / target_fps as f64);
        if elapsed < min_period {
            std::thread::sleep(min_period - elapsed);
        }
    }
}

// helpers ---------------------------------------------------------------

fn busy_like(e: &str) -> bool {
    e.contains("I/O in progress") || e.contains("Device busy") || e.contains("Camera busy")
}

fn pause_busy_ms(base: u64, attempt: u32) {
    std::thread::sleep(Duration::from_millis(base * attempt as u64));
}

/// Lê a árvore de config atual e empurra para a GUI
fn push_tree(cam: &Camera, ev_tx: &Sender<EngineEv>) {
    match cam.config().wait() {
        Ok(root) => {
            let nodes: Vec<CfgNode> = root.children_iter().map(|w| snapshot_node(&w, "")).collect();
            crate::core::log::log("DEBUG", &format!("tree re-enviada ({} na raiz)", nodes.len()));
            send(ev_tx, EngineEv::ConfigTree(nodes));
        }
        Err(e) => {
            crate::core::log::log("ERROR", &format!("get_config: {e}"));
            send(ev_tx, EngineEv::PreviewLost(format!("get_config: {e}")))
        }
    }
}

/// Copia um widget (e filhos recursivamente) para o snapshot serializável
fn snapshot_node(w: &Widget, path: &str) -> CfgNode {
    let name = w.name();
    let full = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
    let label = w.label();
    let ro = w.readonly();
    let info = w.info();

    let kind: CfgKind;
    let mut children: Vec<CfgNode> = Vec::new();
    let value = match w {
        Widget::Group(g) => {
            kind = CfgKind::Section;
            children = g.children_iter().map(|c| snapshot_node(&c, &full)).collect();
            None
        }
        Widget::Text(t) => {
            kind = CfgKind::Text;
            Some(CfgValue::Text(t.value()))
        }
        Widget::Range(r) => {
            kind = CfgKind::Range;
            let (rng, step) = r.range_and_step();
            Some(CfgValue::Range {
                value: r.value(),
                min: *rng.start(),
                max: *rng.end(),
                step,
            })
        }
        Widget::Toggle(t) => {
            kind = CfgKind::Toggle;
            Some(CfgValue::Toggle(t.toggled().unwrap_or(false)))
        }
        Widget::Radio(r) => {
            kind = CfgKind::Radio;
            Some(CfgValue::Radio {
                options: r.choices_iter().collect(),
                current: r.choice(),
            })
        }
        Widget::Button(_) => {
            kind = CfgKind::Button;
            None
        }
        Widget::Date(d) => {
            kind = CfgKind::Date;
            Some(CfgValue::Date(d.timestamp() as i64))
        }
    };

    CfgNode { name: full, label, kind, readonly: ro, info, value, children }
}

/// Aplica valor (string → tipo) ao widget `name` e envia à câmera.
/// `__press__` = clicar Button (foco etc.)
fn apply_config(cam: &Camera, name: &str, value: &str) -> Result<(), String> {
    let root = cam.config().wait().map_err(|e| format!("get_config: {e}"))?;
    let w = find_widget(&root, name)
        .ok_or_else(|| format!("widget {name:?} não encontrado na tree"))?;
    match &w {
        Widget::Button(b) => b.press(cam).map_err(|e| format!("press: {e}")),
        _ => {
            let applied = match &w {
                Widget::Text(t) => t.set_value(value).map_err(|e| e.to_string()),
                Widget::Range(r) => {
                    let v: f32 = value.parse().map_err(|_| "valor numérico inválido")?;
                    r.set_value(v);
                    Ok(())
                }
                Widget::Toggle(t) => {
                    let v = matches!(value, "1") || value.eq_ignore_ascii_case("true");
                    t.set_toggled(v);
                    Ok(())
                }
                Widget::Radio(r) => {
                    r.set_choice(value).map_err(|e| format!("escolha {value:?}: {e}"))
                }
                _ => Ok(()),
            };
            // gp_camera_set_single_config
            applied.and_then(|()| cam.set_config(&w).wait().map_err(|e| format!("set_config: {e}")))
        }
    }
}

/// Busca path `/`-joined na tree (get_child_by_name por segmento —
/// gp_camera_get_single_config só resolve widgets da raiz)
fn find_widget(root: &gphoto2::widget::GroupWidget, path: &str) -> Option<Widget> {
    let mut parts = path.split('/');
    let seg = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    let w = root.get_child_by_name(seg).ok()?;
    if rest.is_empty() {
        return Some(w);
    }
    match w {
        Widget::Group(g) => find_widget(&g, &rest.join("/")),
        _ => None,
    }
}

/// Captura full-res + download para a sessão local (Fase 3).
/// Canon USB: capture_image rejeita se o preview está "no ar" ("I/O in progress")
/// → pausa única antes de 1 tentativa (retry dispara fotos duplicadas!).
fn capture_photo(cam: &Camera, ev_tx: &Sender<EngineEv>) {
    crate::core::log::log("INFO", "captura iniciada");
    send(ev_tx, EngineEv::CaptureBusy);
    std::thread::sleep(Duration::from_millis(900));
    send(ev_tx, EngineEv::CaptureBusy);

    let fp = match cam.capture_image().wait() {
        Ok(fp) => fp,
        Err(e) => {
            crate::core::log::log("ERROR", &format!("capture_image: {e}"));
            send(ev_tx, EngineEv::CaptureDone {
                path: None,
                error: Some(format!("capture: {e}")),
            });
            return;
        }
    };
    let (folder, name) = (fp.folder().into_owned(), fp.name().into_owned());
    let session = session_dir();
    if let Err(e) = std::fs::create_dir_all(&session) {
        send(ev_tx, EngineEv::CaptureDone {
            path: None,
            error: Some(format!("mkdir {session:?}: {e}")),
        });
        return;
    }
    let dest = session.join(&name);
    let dest_s = dest.to_string_lossy().into_owned();
    send(ev_tx, EngineEv::CaptureBusy);
    if let Err(e) = cam.fs().download_to(&folder, &name, &dest).wait() {
        crate::core::log::log("ERROR", &format!("download {folder}/{name}: {e}"));
        send(ev_tx, EngineEv::CaptureDone {
            path: None,
            error: Some(format!("download {folder}/{name}: {e}")),
        });
        return;
    }
    let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if size < 1024 {
        send(ev_tx, EngineEv::CaptureDone {
            path: None,
            error: Some(format!("arquivo suspeito ({size} bytes): {dest_s}")),
        });
        return;
    }
    crate::core::log::log("INFO", &format!("captura ok: {dest_s}"));
    send(ev_tx, EngineEv::CaptureDone { path: Some(dest_s), error: None });
}

/// Pasta da sessão local (downloads de captura): ~/Imagens/Lenvix
pub fn session_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let pics = std::process::Command::new("xdg-user-dir")
        .arg("PICTURES")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{home}/Imagens"));
    std::path::PathBuf::from(pics).join("Lenvix")
}

fn send(ev_tx: &Sender<EngineEv>, ev: EngineEv) {
    let _ = ev_tx.send_timeout(ev, Duration::from_secs(2));
}

/// Decodifica JPEG (MJPEG) → RGB. Retorna (w, h, bytes)
fn decode_rgb(jpeg: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(jpeg));
    if decoder.decode_headers().is_err() {
        return None;
    }
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    match decoder.decode() {
        Ok(rgb) => Some((w, h, rgb)),
        Err(_) => None,
    }
}
