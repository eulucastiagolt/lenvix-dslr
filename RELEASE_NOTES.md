# Lenvix v0.5.0 (Beta) — Release Notes

Lenvix turns your DSLR or mirrorless camera into a high-performance virtual webcam and recording studio on Linux, powered by Rust, GTK4/Adwaita, and GStreamer.

---

## 🌟 What's New in v0.5.0

### 🎭 Virtual Background & Blur (Google Meet Style)
- **Real-Time Person Segmentation:**
  - Integrated neural network human segmentation powered by Google's MediaPipe Selfie Segmentation.
  - 100% pure Rust inference via `tract-onnx`: no external C/C++ dynamic libraries, no CUDA required, and works entirely offline.
  - Lightning-fast CPU inference: ~4.8ms per frame, easily sustaining 30 to 60 FPS in real time.
- **Configurable Background Blur:**
  - Soft portrait depth-of-field effect (Bokeh).
  - Adjustable blur density slider from 1 (subtle) to 100 (heavy privacy blur).
  - High-performance multi-pass separable box blur with linear $O(1)$ running-sum sampling, taking under 1ms per frame.
- **Custom Image Background Replacement:**
  - Replace your background with any custom picture (PNG, JPEG, WebP).
  - Native file picker dialog with automatic aspect-fill scaling and centered cropping.
  - Image texture caching in memory for zero disk I/O overhead while streaming.
- **Universal Synchronization:**
  - Virtual background effects apply simultaneously to the live GUI preview, the V4L2 virtual webcam output (Google Meet, Zoom, OBS, Discord), and local video recordings.
- **Effortless UI Integration:**
  - Dedicated "Virtual Background & Blur" button in the header bar with an interactive Google Meet-style popover for quick toggles and slider adjustments on the fly.
  - Full configuration persistence in `~/.config/lenvix/settings.conf`.

---

## 📦 Downloads & Installation

### 🐧 AppImage (Universal Linux)
Portable binary that runs on almost any modern Linux distribution:
```bash
chmod +x Lenvix-0.5.0-x86_64.AppImage
./Lenvix-0.5.0-x86_64.AppImage
```

### 📦 Debian / Ubuntu / Linux Mint (`.deb`)
```bash
sudo apt install ./lenvix_0.5.0_amd64.deb
```

### 🎩 Fedora / RHEL / openSUSE (`.rpm`)
```bash
sudo dnf install ./lenvix-0.5.0-1.x86_64.rpm
```

### 📁 Tarball (`.tar.gz`)
Standalone archive with installer script:
```bash
tar -xzf lenvix-v0.5.0-linux-x86_64.tar.gz
cd lenvix-v0.5.0-linux-x86_64
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
