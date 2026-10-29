//! Gravação de vídeo do stream ao vivo com captura de áudio (FR-17b / Fase 4).
//!
//! Grava o pipeline RGB em formatos populares (MP4, MKV, WebM, MOV, AVI via FFmpeg)
//! sincronizado pelo tempo real da câmera/wallclock e áudio capturado via Pulse/PipeWire/ALSA.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct VideoRecorder {
    path: PathBuf,
    started_at: Instant,
    frames_count: u64,
    width: u32,
    height: u32,
    backend: RecorderBackend,
}

enum RecorderBackend {
    Ffmpeg { child: Child },
    Fallback { file: std::fs::File },
}

impl VideoRecorder {
    pub fn start(
        dest_dir: &Path,
        width: u32,
        height: u32,
        audio_source: &str,
        format_ext: &str,
    ) -> Result<Self, String> {
        let _ = std::fs::create_dir_all(dest_dir);
        let now = file_timestamp();

        let has_ffmpeg = Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let ext = match format_ext.to_ascii_lowercase().as_str() {
            "mkv" => "mkv",
            "webm" => "webm",
            "mov" => "mov",
            "avi" => "avi",
            _ => "mp4",
        };

        if has_ffmpeg {
            let path = dest_dir.join(format!("lenvix_video_{now}.{ext}"));
            let mut cmd = Command::new("ffmpeg");
            cmd.arg("-y")
                .arg("-use_wallclock_as_timestamps")
                .arg("1")
                .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{width}x{height}"), "-i", "-"]);

            let audio_active = !audio_source.is_empty() && audio_source != "none" && audio_source != "disabled";
            if audio_active {
                if audio_source.starts_with("hw:") {
                    cmd.args(["-f", "alsa", "-i", audio_source]);
                } else {
                    cmd.args(["-f", "pulse", "-i", audio_source]);
                }
            }

            // Codecs baseados na extensão
            match ext {
                "webm" => {
                    cmd.args(["-c:v", "libvpx-vp9", "-b:v", "2M"]);
                    if audio_active {
                        cmd.args(["-c:a", "libopus", "-b:a", "128k"]);
                    }
                }
                "avi" => {
                    cmd.args(["-c:v", "mpeg4", "-q:v", "3"]);
                    if audio_active {
                        cmd.args(["-c:a", "mp3", "-b:a", "192k"]);
                    }
                }
                "mkv" => {
                    cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]);
                    if audio_active {
                        cmd.args(["-c:a", "aac", "-b:a", "192k"]);
                    }
                }
                _ => {
                    // mp4 / mov
                    cmd.args([
                        "-c:v", "libx264",
                        "-preset", "ultrafast",
                        "-pix_fmt", "yuv420p",
                        "-movflags", "+faststart",
                    ]);
                    if audio_active {
                        cmd.args(["-c:a", "aac", "-b:a", "192k"]);
                    }
                }
            }

            if audio_active {
                cmd.arg("-shortest");
            }

            cmd.arg(path.to_str().unwrap_or("output.mp4"));
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            let child = cmd
                .spawn()
                .map_err(|e| format!("falha ao iniciar ffmpeg: {e}"))?;

            Ok(Self {
                path,
                started_at: Instant::now(),
                frames_count: 0,
                width,
                height,
                backend: RecorderBackend::Ffmpeg { child },
            })
        } else {
            let path = dest_dir.join(format!("lenvix_video_{now}.raw"));
            let file = std::fs::File::create(&path)
                .map_err(|e| format!("falha ao criar arquivo de gravação: {e}"))?;
            Ok(Self {
                path,
                started_at: Instant::now(),
                frames_count: 0,
                width,
                height,
                backend: RecorderBackend::Fallback { file },
            })
        }
    }

    pub fn push_frame(&mut self, rgb: &[u8]) -> Result<(), String> {
        match &mut self.backend {
            RecorderBackend::Ffmpeg { child } => {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(rgb).map_err(|e| format!("erro ao gravar frame: {e}"))?;
                    self.frames_count += 1;
                    Ok(())
                } else {
                    Err("stdin do gravador fechado".into())
                }
            }
            RecorderBackend::Fallback { file } => {
                file.write_all(rgb).map_err(|e| format!("erro ao gravar frame: {e}"))?;
                self.frames_count += 1;
                Ok(())
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_count
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn finish(self) -> Result<(PathBuf, Duration, u64), String> {
        let duration = self.started_at.elapsed();
        match self.backend {
            RecorderBackend::Ffmpeg { mut child } => {
                drop(child.stdin.take());
                let status = child.wait().map_err(|e| format!("erro ao finalizar gravação: {e}"))?;
                if !status.success() {
                    return Err(format!("ffmpeg finalizou com erro: {status}"));
                }
            }
            RecorderBackend::Fallback { mut file } => {
                let _ = file.flush();
            }
        }
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        Ok((self.path, duration, size))
    }
}

fn file_timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}_{:02}{:02}{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
