# Lenvix v0.4.0 (Beta) — Release Notes

Lenvix turns your DSLR or mirrorless camera into a high-performance virtual webcam and recording studio on Linux, powered by Rust, GTK4/Adwaita, and GStreamer.

---

## 🌟 What's New in v0.4.0

### 🌐 Internationalization (i18n)
- **Multi-language Support:** Built-in catalog support via GNU gettext.
- **Languages Included:**
  - English (`en_US`) — default.
  - Brazilian Portuguese (`pt_BR`) — fully translated UI, dialogs, status indicators, and settings.
- **Language Switcher:** Change interface language on the fly directly in the Settings window without restarting.

### ⚡ Performance & Memory Optimizations
- **Zero-Allocation Color Conversion:**
  - Redesigned `rgb_to_yuyv_into` using a reusable engine scratch buffer.
  - Eliminates per-frame heap churn (~120 MB/s saved at 1080p30 / 60 FPS).
- **Vectorized RGB-to-RGBA Processing:**
  - Utilizes 4-byte chunked mutations (`as_chunks_mut::<4>`) for preview texture generation, optimizing cache locality and compiler auto-vectorization.
- **Camera Settings Cache:**
  - Replaced redundant string lookups with instant macro snapshots (`current_settings!()`), cutting UI thread overhead.

### 👁️ Preview Memory Reclamation & Auto-Suspend (OBS-style)
- **Manual Preview Hide:**
  - Headerbar toggle button and context menu option to hide the live camera preview.
  - Automatically clears GPU and RAM texture memory (`picture.set_paintable(None)`), lowering idle CPU and memory consumption.
- **Smart Auto-Suspend:**
  - When enabled in Settings, the live preview automatically suspends when the Lenvix window is minimized or loses focus.
  - Automatically resumes instantly when the window is brought back to focus.
- **Zero-CPU USB Idle:**
  - USB frame polling via `gphoto2` pauses when the preview is suspended and the virtual webcam/recorder are not actively streaming.

---

## 📦 Downloads & Installation

### 🐧 AppImage (Universal Linux)
Portable binary that runs on almost any modern Linux distribution:
```bash
chmod +x Lenvix-0.4.0-x86_64.AppImage
./Lenvix-0.4.0-x86_64.AppImage
```

### 📦 Debian / Ubuntu / Linux Mint (`.deb`)
```bash
sudo apt install ./lenvix_0.4.0_amd64.deb
```

### 🎩 Fedora / RHEL / openSUSE (`.rpm`)
```bash
sudo dnf install ./lenvix-0.4.0-1.x86_64.rpm
```

### 📁 Tarball (`.tar.gz`)
Standalone archive with installer script:
```bash
tar -xzf lenvix-v0.4.0-linux-x86_64.tar.gz
cd lenvix-v0.4.0-linux-x86_64
sudo ./install.sh
```

---

## 📋 Prerequisites

To enable the virtual webcam loopback device:
```bash
# Ubuntu / Debian
sudo apt install v4l2loopback-dkms v4l2loopback-utils libgphoto2-dev

# Fedora
sudo dnf install v4l2loopback libgphoto2-devel

# Arch Linux
sudo pacman -S v4l2loopback-dkms libgphoto2
```

Load the v4l2loopback kernel module:
```bash
sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="Lenvix Camera" exclusive_caps=1
```

---

## 🤝 Feedback & Contributions
Found a bug or have a suggestion? Open an issue on our [GitHub Issue Tracker](https://github.com/eulucastiagolt/lenvix-dslr/issues).
