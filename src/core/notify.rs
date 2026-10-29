//! Notificações de desktop do sistema operacional (Fase 4).

pub fn notify(title: &str, msg: &str, icon: Option<&str>) {
    let title = title.to_string();
    let msg = msg.to_string();
    let icon = icon.unwrap_or("camera-photo-symbolic").to_string();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("notify-send")
            .args(["-a", "Lenvix", "-i", &icon, &title, &msg])
            .output();
    });
}
