//! Enumeração e detecção de dispositivos de áudio via PulseAudio / PipeWire / ALSA.

use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSource {
    pub id: String,
    pub description: String,
}

/// Lista todas as fontes de captura de áudio disponíveis no sistema.
pub fn list_audio_sources() -> Vec<AudioSource> {
    let mut sources = Vec::new();

    // 1. Tenta listar via pactl (PulseAudio / PipeWire)
    if let Ok(output) = Command::new("pactl")
        .args(["list", "sources"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        let mut cur_name = String::new();
        let mut cur_desc = String::new();
        let mut is_monitor = false;

        for line in text.lines() {
            let trimmed = line.trim();
            if line.starts_with("Source #") {
                if !cur_name.is_empty() && !is_monitor {
                    sources.push(AudioSource {
                        id: cur_name.clone(),
                        description: if cur_desc.is_empty() {
                            cur_name.clone()
                        } else {
                            cur_desc.clone()
                        },
                    });
                }
                cur_name.clear();
                cur_desc.clear();
                is_monitor = false;
            } else if let Some(rest) = trimmed.strip_prefix("Name: ") {
                cur_name = rest.trim().to_string();
                if cur_name.ends_with(".monitor") {
                    is_monitor = true;
                }
            } else if let Some(rest) = trimmed.strip_prefix("Description: ") {
                cur_desc = rest.trim().to_string();
            } else if trimmed.starts_with("Monitor of Sink:") && !trimmed.ends_with("n/a") {
                is_monitor = true;
            }
        }
        if !cur_name.is_empty() && !is_monitor {
            sources.push(AudioSource {
                id: cur_name.clone(),
                description: if cur_desc.is_empty() { cur_name } else { cur_desc },
            });
        }
    }

    // 2. Se pactl não encontrou nada ou falhou, tenta arecord (ALSA puro)
    if sources.is_empty()
        && let Ok(output) = Command::new("arecord")
            .arg("-l")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.starts_with("card ") {
                // Exemplo: card 0: SC3 [fifine SC3], device 0: USB Audio [USB Audio]
                if let Some((card_part, dev_part)) = line.split_once(',') {
                    let card_num = card_part
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .trim_end_matches(':');
                    let dev_num = dev_part
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .trim_end_matches(':');
                    if !card_num.is_empty() && !dev_num.is_empty() {
                        let id = format!("hw:{card_num},{dev_num}");
                        let desc = line.trim().to_string();
                        sources.push(AudioSource { id, description: desc });
                    }
                }
            }
        }
    }

    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_sources() {
        let sources = list_audio_sources();
        println!("Detected sources: {:?}", sources);
    }
}

