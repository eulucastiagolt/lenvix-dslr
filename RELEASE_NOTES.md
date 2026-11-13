# Lenvix v1.0.1 — Release Notes

Lenvix 1.0.1 fixes the webcam authorization dialog on systems where Polkit identifies the request by process ID.

- The virtual webcam authorization dialog opens in front of the Lenvix window. The interface shows when setup starts, waits for authorization, or needs another action.
- Run `lenvix --device 9 --fps 30` to stream without a desktop interface using a prepared `v4l2loopback` device. Use `lenvix --status` to check the kernel module and device.

## 📦 Downloads & Installation

### 🐧 AppImage (Universal Linux)
Portable binary that runs on almost any modern Linux distribution:
```bash
chmod +x Lenvix-1.0.1-x86_64.AppImage
./Lenvix-1.0.1-x86_64.AppImage
```

### 📦 Debian / Ubuntu / Linux Mint (`.deb`)
```bash
sudo apt install ./lenvix_1.0.1_amd64.deb
```

### 🎩 Fedora / RHEL / openSUSE (`.rpm`)
```bash
sudo dnf install ./lenvix-1.0.1-1.x86_64.rpm
```

### 📁 Tarball (`.tar.gz`)
Standalone archive with installer script:
```bash
tar -xzf lenvix-v1.0.1-linux-x86_64.tar.gz
cd lenvix-v1.0.1-linux-x86_64
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
