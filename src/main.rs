//! Lenvix — entry point.
//!
//! Camadas: main → tray (SNI) + core-camera (thread gphoto2) + GUI (GTK4).

use lenvix::{core::camera::CameraEngine, gui::{self, app::activate, UiAction}, tray};


use std::{cell::RefCell, rc::Rc};
use crossbeam_channel::unbounded;
use gtk4::{gio::ApplicationFlags, prelude::*};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Comandos CLI (--status, --help, --version, --add-loopback)
    if args.len() >= 2 {
        match args[1].as_str() {
            "--add-loopback" => {
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
            _ => {}
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

fn print_help() {
    println!("Lenvix — DSLR & Mirrorless Live View & Virtual Webcam");
    println!();
    println!("Uso: lenvix [OPÇÕES]");
    println!();
    println!("Opções:");
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
