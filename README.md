# Lenvix

<p align="center">
  <strong>DSLR & Mirrorless Live View, Camera Controls & Virtual Webcam for Linux</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Language-Rust-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/GUI-GTK4-blue.svg" alt="GTK4" />
  <img src="https://img.shields.io/badge/Backend-libgphoto2-green.svg" alt="libgphoto2" />
  <img src="https://img.shields.io/badge/Webcam-v4l2loopback-purple.svg" alt="v4l2loopback" />
  <img src="https://img.shields.io/badge/Platform-Linux-lightgrey.svg" alt="Linux" />
  <img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg" alt="License" />
</p>

---

**Lenvix** is a high-performance desktop application written in Rust designed to bridge DSLR and mirrorless cameras (Canon, Nikon, Sony, Fuji, etc.) to Linux as professional live webcams with full camera configuration controls, tethered capture, and system tray background execution.

Drawing inspiration from both **Entangle** (precise camera controls, tethering, and shutter life preservation) and **webcamize** (clean in-kernel `v4l2loopback` streaming), Lenvix delivers an all-in-one studio utility for streamers, creators, photographers, and video calls.

---

## Features

### 🎥 Studio-Grade Virtual Webcam
- **Ultra-low latency streaming:** Pure Rust in-memory MJPEG decoding (`zune-jpeg`) and fast SIMD color conversion to `YUYV` written directly to `/dev/videoN`.
- **Wide software compatibility:** Configured with `exclusive_caps=1` so your virtual camera is immediately detected by OBS Studio, Google Meet, Zoom, Discord, Chromium, Firefox, etc.
- **Standby & Disconnect Slate:** Outputs a clean, 16:9 standby frame when the camera is disconnected or idle, ensuring your stream never crashes or shows a broken feed.
- **Aspect Ratio & Framing Control:** Real-time 16:9 native/cropped rendering, digital Zoom (1.0x–4.0x), Pan X/Y adjustments, and horizontal mirroring.

### 🛡️ Smart Collision Avoidance & Dynamic Devices
- **Automatic Kernel Conflict Resolution:** If another application (such as OBS Studio) locks `/dev/video9` at a lower or mismatched resolution (e.g., 1080×1080), Lenvix automatically provisions a new clean virtual device (e.g., `/dev/video10`), applies full 1920×1080, and migrates the stream on-the-fly without downtime.
- **Custom `/dev/videoN` Selection:** Type any custom device number or path directly into the device selector.
- **Instant Device Provisioning:** Click the `+` button to create a dedicated virtual output device at any time.
- **Device Status Indicators:** Clearly shows which virtual outputs are active, in-use, or available.

### 📷 DSLR & Mirrorless Camera Management
- **Entangle-Style Shutter Protection:** Connect and tweak camera settings without keeping the camera sensor exposed. The mechanical shutter/mirror only engages when you explicitly turn on **Live View**.
- **Automatic USB Hotplug Detection:** Automatically discovers connected cameras in the background as soon as they are plugged in via USB.
- **Full Settings Tree:** Adjust ISO, aperture (f-number), shutter speed, exposure compensation, white balance, autofocus drive, metering mode, and color profiles directly from your desktop.
- **Tethered Capture & Session Gallery:** Trigger full-resolution RAW/JPEG photo captures stored to your camera card and local session directory with instant thumbnail previews.

### 🖥️ Native Linux Integration
- **Modern GTK4 UI:** Clean, dark-mode friendly desktop user interface.
- **System Tray (SNI):** Minimizes to the system tray (KDE Plasma, GNOME, etc.) to keep streaming in the background without cluttering your workspace.
- **Zero Heavy Dependencies:** Does not depend on heavy CLI wrapper scripts or external FFmpeg processes; uses direct C bindings to `libgphoto2` and Linux V4L2 ioctls.

---

## Installation

### Prerequisites

You need standard build tools, Rust (edition 2024 / MSRV 1.85+), GTK4, libgphoto2, and the `v4l2loopback` kernel module.

#### Arch Linux / Manjaro / EndeavourOS
```bash
sudo pacman -S base-devel rust cargo git gtk4 libgphoto2 v4l2loopback-dkms
```

#### Ubuntu (24.04+) / Debian (Bookworm+)
```bash
sudo apt update
sudo apt install build-essential cargo git libgtk-4-dev libgphoto2-dev v4l2loopback-dkms
```

#### Fedora (40+)
```bash
sudo dnf install @development-tools cargo git gtk4-devel libgphoto2-devel v4l2loopback
```

---

### Loading `v4l2loopback`

Lenvix manages devices dynamically, but you can load the kernel module initially with:

```bash
sudo modprobe v4l2loopback devices=1 exclusive_caps=1 max_openers=30 video_nr=9 card_label="Lenvix Virtual Camera"
```

To load it automatically at boot, create `/etc/modules-load.d/v4l2loopback.conf`:
```text
v4l2loopback
```
And `/etc/modprobe.d/v4l2loopback.conf`:
```text
options v4l2loopback devices=1 exclusive_caps=1 max_openers=30 video_nr=9 card_label="Lenvix Virtual Camera"
```

---

## Building from Source

```bash
# Clone the repository
git clone https://github.com/your-username/lenvix.git
cd lenvix

# Build release binary
cargo build --release

# Run Lenvix
./target/release/lenvix
```

Optional: install system-wide to `/usr/local/bin`:
```bash
sudo cp target/release/lenvix /usr/local/bin/
```

---

## Usage Guide

1. **Connect your camera via USB:**
   - Turn your camera on and set it to **Manual (M)** or **Movie** mode.
   - If your desktop automounts the camera as a storage volume (GIO/GVFS), unmount it so `libgphoto2` can take exclusive control.
2. **Launch Lenvix:**
   - Your camera will appear automatically in the camera selector.
   - Click **Conectar câmera** (Connect Camera) to initialize communication.
   - Adjust ISO, aperture, shutter speed, and white balance from the configuration panel on the left.
3. **Start Streaming:**
   - Click **Live View** to open the shutter and start the real-time preview.
   - Toggle **Webcam Virtual** on.
   - Open OBS Studio, Discord, or your browser, select `/dev/video9` (or the active Lenvix device), and enjoy high-quality video!

---

## OBS Studio Setup Tips

To avoid V4L2 kernel buffer locks when switching resolutions in OBS:
1. In OBS, add a **Video Capture Device (V4L2)** source.
2. Select `/dev/video9` (or `/dev/video10`).
3. Check the box **"Deactivate when not showing"** (*Desativar quando não estiver visível*).
4. Leave the resolution set to **Device Default** (Automático) so OBS adapts seamlessly to any resolution configured in Lenvix.

---

## Architecture Overview

```
                      +-------------------+
                      |   Camera (USB)    |
                      +---------+---------+
                                | (libgphoto2)
                                v
+---------------------------------------------------------------+
|                        Core Engine                            |
|                                                               |
|  +---------------------+           +------------------------+ |
|  | Background Hotplug  |           |     Camera Control     | |
|  | Detection (1.5s)    |           |  Tree (ISO/Aperture)   | |
|  +---------------------+           +------------------------+ |
|                                                               |
|  +----------------------------------------------------------+ |
|  | Image Pipeline: MJPEG -> RGB -> Transform (Zoom/Pan/Flip) | |
|  +-----------------------------+----------------------------+ |
|                                |                              |
|           +--------------------+-------------------+          |
|           v                                        v          |
|  +-------------------+                    +-----------------+ |
|  | GTK4 GUI Preview  |                    | SIMD RGB->YUYV  | |
|  | (MemoryTexture)   |                    +--------+--------+ |
|  +-------------------+                             |          |
+----------------------------------------------------|----------+
                                                     v
                                            +-----------------+
                                            |  v4l2loopback   |
                                            |  (/dev/videoN)  |
                                            +--------+--------+
                                                     |
                                                     v
                                            +-----------------+
                                            | OBS/Zoom/Chrome |
                                            +-----------------+
```

---

## Contributing

Contributions, bug reports, and feature requests are very welcome!
Feel free to open an issue or submit a pull request.

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

---

## License

Distributed under the **GPL-3.0 License**. See `LICENSE` for more information.
