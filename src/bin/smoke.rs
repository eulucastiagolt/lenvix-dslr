//! Smoke test headless: engine de câmera sem GUI (valida o pipeline FR-01/FR-05).
//!
//! Uso: `smoke`        — 12s de preview e conta frames/fps
//!      `smoke webcam` — liga a webcam virtual, 8s, e valida com v4l2-ctl

use lenvix::core::camera::{CameraEngine, EngineCmd, EngineEv};
use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let webcam_mode = arg == "webcam";
    let config_mode = arg == "config";
    let engine = Rc::new(RefCell::new(CameraEngine::spawn()));
    let ev_rx = engine.borrow().ev_rx.clone();

    let t0 = Instant::now();
    let secs = if webcam_mode {
        60
    } else if config_mode {
        30
    } else {
        12
    };
    let mut frames = 0usize;
    let mut last_fps = 0.0;
    let mut dims = (0u32, 0u32);
    let mut tree_pushed = false;

    if webcam_mode {
        engine.borrow().cmd_tx.send(EngineCmd::SetWebcam(true)).expect("cmd fail");
    }
    if config_mode {
        engine.borrow().cmd_tx.send(EngineCmd::GetConfig).ok();
        engine.borrow().cmd_tx.send(EngineCmd::Capture).ok();
    }

    loop {
        if t0.elapsed() >= Duration::from_secs(secs) {
            break;
        }
        for ev in ev_rx.try_iter() {
            match ev {
                EngineEv::AppState { .. } => {}
                EngineEv::Connected(on) => println!("conectado: {on}"),
                EngineEv::CamerasList { cameras, active } => {
                    println!("câmeras: {cameras:?} ativa: {active:?}");
                }
                EngineEv::CameraOpened { model } => println!("aberta: {model}"),
                EngineEv::Frame { w, h, measured_fps, .. } => {
                    frames += 1;
                    last_fps = measured_fps;
                    dims = (w, h);
                }
                EngineEv::PreviewLost(e) => eprintln!("preview lost: {e}"),
                EngineEv::WebcamStatus { on, path, card, error } => {
                    println!("webcam: {on} {path:?} card={card:?} {error:?}")
                }
                EngineEv::ConfigTree(nodes) => {
                    let flat = count_nodes(&nodes);
                    println!("config tree: {flat} widgets ({} na raiz)", nodes.len());
                    if config_mode && !tree_pushed {
                        tree_pushed = true;
                        // teste no-op de SetConfig: re-aplica o valor corrente de um Radio
                        let radio = first_radio(&nodes);
                        if let Some((name, current)) = radio.clone() {
                            println!("teste set (no-op): {name}={current}");
                            engine
                                .borrow()
                                .cmd_tx
                                .send(EngineCmd::SetConfig { name, value: current })
                                .ok();
                        }
                        // botão (foco) — melhor esforço
                        if let Some(name) = first_button(&nodes) {
                            println!("teste press: {name}");
                            engine.borrow().cmd_tx.send(EngineCmd::PressButton { name }).ok();
                        }
                    }
                }
                EngineEv::CaptureBusy => println!("captura: busy"),
                EngineEv::CaptureDone { path, error } => {
                    println!("captura: {path:?} {error:?}");
                    break;
                }
                EngineEv::Fatal(e) => {
                    eprintln!("FATAL: {e}");
                    std::process::exit(1);
                }
                EngineEv::DevicesList { .. } | EngineEv::LiveViewStatus(_) => {}
            }
        }
    }

    if webcam_mode {
        println!("--- v4l2-ctl /dev/video9 ---");
        let _ = std::process::Command::new("v4l2-ctl")
            .arg("-d/dev/video9")
            .arg("--all")
            .status();
        // desliga a webcam (fecha fd) e sai
        engine.borrow().cmd_tx.send(EngineCmd::SetWebcam(false)).ok();
        std::thread::sleep(Duration::from_secs(2));
    }

    println!("smoke: {frames} frames em {secs}s → {last_fps:.1} fps ({}×{})", dims.0, dims.1);
    engine.borrow_mut().shutdown();
}

fn count_nodes(nodes: &[lenvix::core::config::CfgNode]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
}

fn first_radio(nodes: &[lenvix::core::config::CfgNode]) -> Option<(String, String)> {
    for n in nodes {
        if !n.readonly
            && let Some(current) = n.value.as_ref().and_then(|v| match v {
                lenvix::core::config::CfgValue::Radio { current, .. } => Some(current.clone()),
                _ => None,
            })
        {
            let prio = n.name.contains("reviewtime");
            if prio || n.kind == lenvix::core::config::CfgKind::Radio {
                return Some((n.name.clone(), current.clone()));
            }
        }
        if let Some(c) = first_radio(&n.children) {
            return Some(c);
        }
    }
    None
}

fn first_button(nodes: &[lenvix::core::config::CfgNode]) -> Option<String> {
    for n in nodes {
        if n.name.contains("autofocusdrive") {
            return Some(n.name.clone());
        }
        if let Some(c) = first_button(&n.children) {
            return Some(c);
        }
    }
    None
}
