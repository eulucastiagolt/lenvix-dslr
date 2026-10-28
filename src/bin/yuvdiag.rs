//! Diagnóstico de cor (paralelo): writer e leitor em threads concorrentes.
//! A) decode direto do JPEG (referência)  B) RGB→YUYV→loopback→leitor ffmpeg.

use lenvix::core::{stream::ensure_loopback, yuv::rgb_to_yuyv};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn mean_channels(png: &str) -> [f64; 3] {
    // decodifica PNG → rawvideo rgb24 e média por canal
    let out = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-i", png, "-f", "rawvideo", "-pix_fmt", "rgb24",
            "-y", "/tmp/opencode/rgbcheck.raw",
        ])
        .output()
        .expect("ffmpeg run");
    assert!(out.status.success(), "ffmpeg falhou: {out:?}");
    let raw = fs::read("/tmp/opencode/rgbcheck.raw").expect("raw");
    let (mut r, mut g, mut b) = (0f64, 0f64, 0f64);
    let n = raw.len() / 3;
    for v in raw.as_chunks::<3>().0 {
        r += v[0] as f64;
        g += v[1] as f64;
        b += v[2] as f64;
    }
    [r / n as f64, g / n as f64, b / n as f64]
}

fn main() {
    let jpeg = fs::read("/tmp/lenvix-poc-preview.jpeg").expect("jpeg referência");

    // 1) referência: decode via ffmpeg
    fs::write("/tmp/opencode/ref.jpg", &jpeg).unwrap();
    let st = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", "/tmp/opencode/ref.jpg", "-frames:v", "1", "-y", "/tmp/opencode/ref.png"])
        .status().unwrap();
    assert!(st.success());
    let ref_mean = mean_channels("/tmp/opencode/ref.png");
    println!("A) referência (JPEG→ffmpeg): R={:.1} G={:.1} B={:.1}", ref_mean[0], ref_mean[1], ref_mean[2]);

    // 2) pipeline: jpeg → rgb → yuyv → device → leitor ffmpeg (concorrente)
    let mut dec = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(&jpeg[..]));
    dec.decode_headers().unwrap();
    let info = dec.info().unwrap();
    let (w, h) = (info.width as u32, info.height as u32);
    let rgb = dec.decode().unwrap();
    println!("jpeg {w}x{h}");

    let (done_tx, done_rx) = std::sync::mpsc::channel::<bool>();
    thread::spawn(move || {
        let Ok(mut writer) = ensure_loopback(9, "Lenvix") else {
            done_tx.send(false).unwrap();
            return;
        };
        let yuyv = rgb_to_yuyv(&rgb, w, h);
        if writer.ensure_format(w, h).is_err() {
            done_tx.send(false).unwrap();
            return;
        }
        let t0 = Instant::now();
        let mut ok = true;
        while t0.elapsed().as_secs_f32() < 6.0 {
            if writer.write_frame(&yuyv).is_err() {
                ok = false;
                break;
            }
            thread::sleep(Duration::from_millis(33));
        }
        done_tx.send(ok).unwrap();
    });

    // leitor "concorrente" (como OBS) — parte após 2s de frames
    thread::sleep(Duration::from_secs(2));
    let st = Command::new("timeout")
        .args(["8", "ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "v4l2", "-i", "/dev/video9", "-frames:v", "1", "-y", "/tmp/opencode/loop.png"])
        .status().expect("reader ffmpeg");
    if !st.success() {
        eprintln!("leitor ffmpeg falhou: {st:?}");
        return;
    }

    match done_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(true) => {}
        Ok(false) => eprintln!("writer parou antes"),
        Err(e) => eprintln!("writer sem resposta: {e}"),
    }

    let loop_mean = mean_channels("/tmp/opencode/loop.png");
    println!("B) pipeline (YUYV→loopback→ffmpeg): R={:.1} G={:.1} B={:.1}", loop_mean[0], loop_mean[1], loop_mean[2]);
    let dr = loop_mean[0] - ref_mean[0];
    let dg = loop_mean[1] - ref_mean[1];
    let db = loop_mean[2] - ref_mean[2];
    println!("delta R={dr:+.1} G={dg:+.1} B={db:+.1}");
}
