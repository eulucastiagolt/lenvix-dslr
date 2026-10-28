//! Persistência do estado do aplicativo em `~/.config/lenvix/settings.conf`
//! (Fase 3c). Uma linha por chave `chave=valor`; arquivo ausente = defaults.
//! Escrita atômica (tmp + rename) e silenciosa (settings nunca derrubam o app).

use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub fps: u32,
    pub webcam: bool,
    pub mirror: bool,
    pub zoom: f32,
    pub pan_x: i16,
    pub pan_y: i16,
    pub out: Option<(u32, u32)>,
    pub autoconnect: bool,
    pub video_device: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fps: 30,
            webcam: true,
            mirror: false,
            zoom: 1.0,
            pan_x: 0,
            pan_y: 0,
            out: Some((1920, 1080)), // 16:9 por padrão!
            autoconnect: false,
            video_device: 9,
        }
    }
}

/// Pasta de config do app: $XDG_CONFIG_HOME/lenvix (fallback ~/.config/lenvix,
/// /tmp quando HOME indisponível). Usada também pelo log de disco.
pub fn dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.config")))
        .unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("lenvix")
}

pub fn path() -> PathBuf {
    dir().join("settings.conf")
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(txt) = std::fs::read_to_string(path()) else {
        return s;
    };
    let mut ow: Option<u32> = None;
    let mut oh: Option<u32> = None;
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "fps" => {
                if let Ok(f) = v.parse::<u32>() {
                    s.fps = f.clamp(1, 60);
                }
            }
            "webcam" => {
                if let Ok(b) = v.parse::<bool>() {
                    s.webcam = b;
                }
            }
            "mirror" => {
                if let Ok(b) = v.parse::<bool>() {
                    s.mirror = b;
                }
            }
            "zoom" => {
                if let Ok(z) = v.parse::<f32>() {
                    s.zoom = z.clamp(1.0, 4.0);
                }
            }
            "pan_x" => {
                if let Ok(p) = v.parse::<i16>() {
                    s.pan_x = p.clamp(-100, 100);
                }
            }
            "pan_y" => {
                if let Ok(p) = v.parse::<i16>() {
                    s.pan_y = p.clamp(-100, 100);
                }
            }
            "out_w" => ow = v.parse::<u32>().ok().map(|w| w & !1),
            "out_h" => oh = v.parse::<u32>().ok().map(|h| h & !1),
            "autoconnect" => {
                if let Ok(b) = v.parse::<bool>() {
                    s.autoconnect = b;
                }
            }
            "video_device" => {
                if let Ok(n) = v.parse::<u32>() {
                    s.video_device = n;
                }
            }
            _ => {}
        }
    }
    s.out = ow.zip(oh).filter(|(w, h)| *w >= 64 && *h >= 64).or(Some((1920, 1080)));
    s
}

pub fn save(s: &Settings) {
    let dir = dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        crate::core::log::log("WARN", &format!("settings: mkdir {dir:?}: {e}"));
        return;
    }
    let txt = format!(
        "# lenvix — configurações do app (gerado automaticamente)\n\
         fps={}\nwebcam={}\nmirror={}\nzoom={:.3}\npan_x={}\npan_y={}\n\
         out_w={}\nout_h={}\nautoconnect={}\nvideo_device={}\n",
        s.fps,
        s.webcam,
        s.mirror,
        s.zoom,
        s.pan_x,
        s.pan_y,
        s.out.map_or(0, |(w, _)| w),
        s.out.map_or(0, |(_, h)| h),
        s.autoconnect,
        s.video_device
    );
    let tmp = dir.join("settings.conf.tmp");
    if let Err(e) = std::fs::write(&tmp, txt) {
        crate::core::log::log("WARN", &format!("settings: write {tmp:?}: {e}"));
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path()) {
        crate::core::log::log("WARN", &format!("settings: rename: {e}"));
    }
}
