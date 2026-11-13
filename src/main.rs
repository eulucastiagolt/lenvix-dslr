//! Lenvix — entry point.
//!
//! Camadas: main → tray (SNI) + core-camera (thread gphoto2) + GUI (GTK4).

use lenvix::{
    core::{camera::{CameraEngine, EngineEv}, i18n, settings},
    gui::{self, app::activate},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static STOP_STREAM: AtomicBool = AtomicBool::new(false);

extern "C" fn stop_stream(_: libc::c_int) {
    STOP_STREAM.store(true, Ordering::Relaxed);
}
use gtk4::{gio::ApplicationFlags, prelude::*};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Comandos CLI (--status, --help, --version, --add-loopback)
    if args.len() >= 2 {
        match args[1].as_str() {
            "--add-loopback" => {
                let Some(nr) = args.get(2).and_then(|s| s.parse::<i32>().ok()).filter(|nr| *nr >= -1) else {
                    eprintln!("Invalid virtual webcam device number.");
                    std::process::exit(1);
                };
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
            "--status" => {
                print_status();
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!("Lenvix {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                let result = parse_stream_args(&args[1..]).and_then(|(device, fps)| {
                    i18n::init(Some(&settings::load().language));
                    stream_headless(device, fps)
                });
                if let Err(error) = result {
                    eprintln!("Lenvix: {error}");
                    std::process::exit(1);
                }
                return;
            }
        }
    }

    // i18n: carrega idioma explícito ou detecta via ambiente (fallback inglês)
    i18n::init(Some(&lenvix::core::settings::load().language));

    let app = gtk4::Application::new(Some(gui::app::APP_ID), ApplicationFlags::NON_UNIQUE);
    app.connect_activate(activate);

    let _ = app.run();
}

fn parse_stream_args(args: &[String]) -> Result<(u32, u32), String> {
    let saved = settings::load();
    let mut device = saved.video_device;
    let mut fps = saved.fps.max(1);
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let range = match flag.as_str() {
            "--device" => 0..=i32::MAX as u32,
            "--fps" => 1..=u32::MAX,
            _ => return Err(format!("Opção desconhecida: {flag}. Use --help.")),
        };
        let value = args.next()
            .and_then(|arg| arg.parse().ok())
            .filter(|value| range.contains(value))
            .ok_or_else(|| format!("Valor inválido para {flag}. Use --help."))?;
        if flag == "--device" {
            device = value;
        } else {
            fps = value;
        }
    }
    Ok((device, fps))
}

fn stream_headless(device: u32, fps: u32) -> Result<(), String> {
    unsafe {
        libc::signal(libc::SIGINT, stop_stream as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop_stream as *const () as libc::sighandler_t);
    }

    let mut engine = CameraEngine::spawn_headless(device, fps);
    let result = loop {
        if STOP_STREAM.load(Ordering::Relaxed) {
            break Ok(());
        }
        match engine.ev_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(EngineEv::WebcamStatus { on: true, path, error, .. }) => {
                if let Some(error) = error {
                    eprintln!("Lenvix: {error}");
                } else if let Some(path) = path {
                    println!("Webcam virtual: {path} (até {fps} FPS). Aguardando câmera USB.");
                }
            }
            Ok(EngineEv::WebcamStatus { on: false, error, .. }) => {
                break Err(error.unwrap_or_else(|| "Webcam virtual desligada.".into()));
            }
            Ok(EngineEv::CameraOpened { model }) => println!("Câmera conectada: {model}"),
            Ok(EngineEv::Connected(false)) => println!("Aguardando câmera USB."),
            Ok(EngineEv::Fatal(error)) => break Err(error),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                break Err("O motor da câmera foi encerrado.".into());
            }
            _ => {}
        }
    };
    engine.shutdown();
    result
}

fn print_help() {
    println!("Lenvix — DSLR & Mirrorless Live View & Virtual Webcam");
    println!();
    println!("Uso: lenvix [OPÇÕES]");
    println!();
    println!("Opções:");
    println!("  --device N        Transmite sem interface para /dev/videoN");
    println!("  --fps N           Define o FPS alvo da transmissão sem interface");
    println!("  --status          Exibe relatório de diagnóstico do sistema e sai");
    println!("  -v, --version     Exibe versão do aplicativo e sai");
    println!("  -h, --help        Exibe esta mensagem de ajuda e sai");
    println!("  --add-loopback N  (Interno) Provisiona dispositivo /dev/videoN");
}

fn print_status() {
    println!("=== Lenvix v{} — Relatório de Status ===", env!("CARGO_PKG_VERSION"));
    println!();

    // 1. Configurações
    let s = lenvix::core::settings::load();
    let cfg_path = lenvix::core::settings::path();
    println!("[Configurações]");
    println!("  Arquivo: {}", cfg_path.display());
    println!("  FPS alvo: {}", s.fps);
    println!("  Webcam virtual ativa: {}", s.webcam);
    println!("  Dispositivo padrão: /dev/video{}", s.video_device);
    println!("  Espelhamento ativo: {}", s.mirror);
    println!(
        "  Resolução alvo: {}",
        s.out
            .map(|(w, h)| format!("{w}x{h}"))
            .unwrap_or_else(|| "Nativa".into())
    );
    println!("  Auto-conectar USB: {}", s.autoconnect);
    println!();

    // 2. Dispositivos v4l2loopback
    println!("[Webcam Virtual / v4l2loopback]");
    let module_loaded = std::path::Path::new("/sys/module/v4l2loopback").exists();
    let control_available = std::path::Path::new("/dev/v4l2loopback").exists();
    println!("  Módulo carregado: {}", if module_loaded { "sim" } else { "não" });
    println!("  Dispositivo de controle disponível: {}", if control_available { "sim" } else { "não" });
    let devs = lenvix::core::stream::list_loopback_devices_detailed();
    if devs.is_empty() {
        println!("  Nenhum dispositivo /dev/videoN encontrado.");
    } else {
        for d in devs {
            let in_use = if d.in_use { "ocupado" } else { "livre" };
            println!("  /dev/video{} — \"{}\" ({in_use})", d.nr, d.card);
        }
    }
    println!();

    // 3. Câmeras USB
    println!("[Câmeras USB / libgphoto2]");
    match gphoto2::Context::new() {
        Ok(ctx) => match ctx.list_cameras().wait() {
            Ok(list) => {
                let cameras: Vec<_> = list.collect();
                if cameras.is_empty() {
                    println!("  Nenhuma câmera detectada.");
                } else {
                    for d in cameras {
                        println!("  • {} [{}]", d.model, d.port);
                    }
                }
            }
            Err(e) => println!("  Erro ao listar câmeras: {e}"),
        },
        Err(e) => println!("  Falha ao inicializar libgphoto2: {e}"),
    }
    println!();

    // 4. Sessão
    let session = lenvix::core::camera::session_dir();
    println!("[Sessão e Armazenamento]");
    println!("  Diretório: {}", session.display());
    if let Ok(rd) = std::fs::read_dir(&session) {
        let count = rd.filter_map(|e| e.ok()).count();
        println!("  Arquivos salvos: {count}");
    } else {
        println!("  Arquivos salvos: 0 (diretório não criado ainda)");
    }
    println!();
}
