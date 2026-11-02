# Lenvix v0.6.0 (Beta) — Release Notes

Lenvix turns your DSLR or mirrorless camera into a high-performance virtual webcam and recording studio on Linux, powered by Rust, GTK4/Adwaita, and GStreamer.

---

## 🌟 What's New in v0.6.0

### 🚀 Hardware Acceleration (GPU)
- **Zero-CPU Video Encoding via VA-API:**
  - Integrated hardware acceleration detection using Linux Direct Rendering Infrastructure (`/dev/dri/renderD128`).
  - Seamlessly utilizes `h264_vaapi` hardware encoders for AMD Radeon (Mesa Gallium) and Intel GPUs.
  - Reduces CPU consumption to virtually 0% during full HD / 4K local video recording sessions.
  - Automatic fallback to high-speed CPU encoding (`libx264 -preset ultrafast`) when hardware encoding is unavailable or disabled.
- **Configurable Settings Toggle:**
  - New switch in **Settings → Application & Performance**: *"Hardware acceleration (GPU)"*.
  - Allows users to freely enable or disable hardware acceleration anytime with instant persistence in `~/.config/lenvix/settings.conf`.

### 🛡️ Background Execution Control
- **Keep Running in Background Toggle:**
  - Added new configuration option in **Settings → Application & Performance**: *"Keep running in background when closed"*.
  - When enabled (default): Closing the main window keeps the virtual webcam active in the system tray.
  - When disabled: Closing the main window cleanly shuts down the camera engine, frees USB and V4L2 handles, and terminates the application completely.

### 🎭 Virtual Background & Blur (Google Meet Style)
- **High-Performance Multi-Threaded Processing:**
  - Integrated parallel row processing across all available CPU cores using Rayon.
  - Eliminated 6.2 MB intermediate buffer allocations per frame, fusing upscale and alpha blending directly in-place.
  - Temporal frame cadence: leverages human body temporal coherence (Google Meet / Zoom pattern) to reduce neural network inference overhead by 50% without visual degradation.
  - Slashes 1080p frame processing time from 23.1ms down to 7.7ms (a 300% performance boost).
- **Real-Time Person Segmentation:**
  - Integrated neural network human segmentation powered by Google's MediaPipe Selfie Segmentation.
  - 100% pure Rust inference via `tract-onnx`: no external C/C++ dynamic libraries, no CUDA required, and works entirely offline.
- **Configurable Background Blur:**
  - Soft portrait depth-of-field bokeh effect with density slider from 1 (subtle) to 100 (heavy privacy blur).
  - High-performance multi-pass separable box blur with linear $O(1)$ running-sum sampling (<1ms per frame).
- **Custom Image Background Replacement:**
  - Replace your background with any custom picture (PNG, JPEG, WebP).
  - Native file picker dialog with automatic aspect-fill scaling and centered cropping.
- **Universal Synchronization:**
  - Virtual background effects apply simultaneously to the live GUI preview, the V4L2 virtual webcam output (Google Meet, Zoom, OBS, Discord), and local video recordings.

### 🐛 Bug Fixes & Polish
- **Physical Shutter & Mirror Close on Stop Live View:** Turning off Live View, disconnecting the camera, or shutting down the application now explicitly sends PTP viewfinder control commands (`viewfinder`, `eosviewfinder`, `actions/viewfinder`) to immediately close the camera's physical shutter and lower the mirror, saving battery and eliminating sensor heat.
- **Pango Markup Formatting:** Fixed unescaped ampersand entity warnings in header bar and settings labels (`Virtual Background &amp; Blur`, `Application &amp; Performance`).
- **Packaging & CI Improvements:** Automated version synchronization from `Cargo.toml` across `.deb`, `.rpm`, `.AppImage`, and Arch packaging scripts.

---

## 📦 Downloads & Installation

### 🐧 AppImage (Universal Linux)
Portable binary that runs on almost any modern Linux distribution:
```bash
chmod +x Lenvix-0.6.0-x86_64.AppImage
./Lenvix-0.6.0-x86_64.AppImage
```

### 📦 Debian / Ubuntu / Linux Mint (`.deb`)
```bash
sudo apt install ./lenvix_0.6.0_amd64.deb
```

### 🎩 Fedora / RHEL / openSUSE (`.rpm`)
```bash
sudo dnf install ./lenvix-0.6.0-1.x86_64.rpm
```

### 📁 Tarball (`.tar.gz`)
Standalone archive with installer script:
```bash
tar -xzf lenvix-v0.6.0-linux-x86_64.tar.gz
cd lenvix-v0.6.0-linux-x86_64
sudo ./install.sh
```

---

## 📋 Prerequisites

To enable virtual webcam output in conferencing applications (Google Meet, Zoom, Discord, OBS), install `v4l2loopback`:

- **Ubuntu / Debian / Mint:**
  ```bash
  sudo apt install v4l2loopback-dkms
  ```
- **Fedora:**
  ```bash
  sudo dnf install v4l2loopback
  ```
- **Arch / Manjaro:**
  ```bash
  sudo pacman -S v4l2loopback-dkms
  ```

---

## 🤝 Contributing & Feedback
Issues, bug reports, and pull requests are welcome on GitHub:
👉 [https://github.com/eulucastiagolt/lenvix-dslr](https://github.com/eulucastiagolt/lenvix-dslr)
