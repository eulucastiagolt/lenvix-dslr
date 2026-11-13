//! core-stream: webcam virtual via v4l2loopback — pipeline fiel ao webcamize.
//!
//! 1. Modulo `v4l2loopback` carregado com `devices=N exclusive_caps=1`
//!    (exclusive_caps expõe apenas "capture": compat. Chromium/Zoom — FR-21).
//! 2. `/dev/videoN` aberto com open(); scelto device validado.
//! 3. 1.º frame define o formato: ioctl VIDIOC_S_FMT em **YUYV** (fiel webcamize).
//! 4. Frames escritos com write() no fd; rate-limit feita pelo core-camera.

use std::{
    cell::OnceCell,
    fs::{File, OpenOptions},
    io::Write,
    os::fd::AsRawFd,
    process::{Command, Stdio},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

use crate::core::i18n::{tr, trf};

thread_local! {
    static AUTHORIZATION: OnceCell<(Result<(), String>, Arc<AtomicBool>)> = const { OnceCell::new() };
}

pub(super) fn set_authorization(result: Result<(), String>, stopping: Arc<AtomicBool>) {
    AUTHORIZATION.with(|auth| {
        let _ = auth.set((result, stopping));
    });
}

/// errno EINVAL (22) via __errno_location indireto (sem crate libc p/ isso)
fn errno_invalid() -> i32 {
    22
}

pub const V4L2_PIX_FMT_YUYV: u32 = fourcc(b"YUYV");

#[inline(always)]
const fn fourcc(s: &[u8; 4]) -> u32 {
    (s[0] as u32) | ((s[1] as u32) << 8) | ((s[2] as u32) << 16) | ((s[3] as u32) << 24)
}

// ---- ABI linux/videodev2.h (x86_64) ----

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct V4l2PixFormat {
    pub width: u32,
    pub height: u32,
    pub pixelformat: u32,
    pub field: u32,
    pub bytesperline: u32,
    pub sizeimage: u32,
    pub colorspace: u32,
    pub priv_: u32,
    pub flags: u32,
    pub ycbcr_enc: u32,
    pub quantization: u32,
    pub xfer_func: u32,
}

/// struct v4l2_format = { u32 type; union { struct v4l2_pix_format pix; … } };
/// union alinhada a 8 (padd 4 bytes após `type`); altura tamanho 208 bytes
/// (medido: `sizeof(struct v4l2_format)=208`, `offsetof(fmt.pix)=8` neste kernel).
/// ATENÇÃO: validado por ioctl (ENOTTY se errado) — ver src/bin/abi_check.rs? (gcc check)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct V4l2Format {
    pub ty: u32,
    _pad: u32,
    pub pix: V4l2PixFormat,
    _rest: [u8; ABI_FMT_SIZE - 8 - std::mem::size_of::<V4l2PixFormat>()],
}

/// sizeof(struct v4l2_format) no ABI local (videodev2.h — medido 208)
const ABI_FMT_SIZE: usize = 208;

impl Default for V4l2Format {
    fn default() -> Self {
        Self {
            ty: 0,
            _pad: 0,
            pix: V4l2PixFormat::default(),
            _rest: [0; ABI_FMT_SIZE - 8 - std::mem::size_of::<V4l2PixFormat>()],
        }
    }
}

// _IOC(_dir, type, nr, size) — videodev2.h
const _IOC_NRSHIFT: u32 = 0;
const _IOC_NRBITS: u32 = 8;
const _IOC_TYPESHIFT: u32 = 8;
const _IOC_TYPEBITS: u32 = 8;
const _IOC_SIZESHIFT: u32 = 16;
const _IOC_SIZEBITS: u32 = 14;
const _IOC_DIRSHIFT: u32 = 30;
const _IOC_WRITE: u32 = 1;
const _IOC_READ: u32 = 2;

const fn ioc(dir: u32, ty: u8, nr: u8, size: usize) -> u32 {
    ((dir & 0x3) << _IOC_DIRSHIFT)
        | (((size as u32) & 0x3eff) << _IOC_SIZESHIFT)
        | ((ty as u32 & 0xff) << _IOC_TYPESHIFT)
        | ((nr as u32 & 0xff) << _IOC_NRSHIFT)
}

const fn iowr(ty: u8, nr: u8, size: usize) -> u32 {
    ioc(_IOC_READ | _IOC_WRITE, ty, nr, size)
}

/// VIDIOC_S_FMT = _IOWR('V', 5, struct v4l2_format)
const VIDIOC_S_FMT: u32 = iowr(b'V', 5, std::mem::size_of::<V4l2Format>());
/// VIDIOC_G_FMT = _IOWR('V', 4, struct v4l2_format)
#[allow(dead_code)] // hoje só usamos o struct devolvido pelo S_FMT; G_FMT fica p/ diagnóstico
const VIDIOC_G_FMT: u32 = iowr(b'V', 4, std::mem::size_of::<V4l2Format>());
/// VIDIOC_QUERYCAP = _IOR('V', 0, struct v4l2_capability) — 104 bytes:
/// driver[16]+card[32]+bus_info[32]+3×u32+3×u32(reserved)
const VIDIOC_QUERYCAP: u32 = ioc(_IOC_READ, b'V', 0, 104);

/// V4L2_BUF_TYPE_VIDEO_OUTPUT = 2
const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
/// V4L2_FIELD_NONE = 1; V4L2_COLORSPACE_SRGB = 1; V4L2_QUANTIZATION_DEFAULT=3? (usar 0=DEFAULT)
const V4L2_FIELD_NONE: u32 = 1;
const V4L2_COLORSPACE_DEFAULT: u32 = 0;

/// controles do módulo v4l2loopback (IDs observados em `v4l2-ctl --all`):
/// keep_format congela o formato atual — S_FMT de leitores (probing do OBS)
/// passa a falhar e o leitor usa G_FMT (nosso YUYV). "sustain_framerate" etc. depois.
const V4L2_CTRL_ID_KEEP_FORMAT: u32 = 0x0098f900;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct V4l2Control {
    pub id: u32,
    pub value: i32,
}

/// VIDIOC_S_CTRL = _IOWR('V', 28, struct v4l2_control)  (G_CTRL=27)
const VIDIOC_S_CTRL: u32 = iowr(b'V', 28, std::mem::size_of::<V4l2Control>());

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct V4l2LoopbackConfig {
    pub output_nr: i32,
    pub unused: i32,
    pub card_label: [u8; 32],
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
    pub max_buffers: i32,
    pub max_openers: i32,
    pub debug: i32,
    pub announce_all_caps: i32,
}

pub const V4L2LOOPBACK_CTL_ADD: u32 = 0x4048_7e01;

/// Informações detalhadas de um dispositivo loopback (/dev/videoN)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackDevInfo {
    pub nr: u32,
    pub card: String,
    pub in_use: bool,
    pub format: Option<String>,
}

/// Escritor do device de webcam virtual. Dono único do fd (requisito 4.1 core-stream).
pub struct LoopbackWriter {
    file: File,
    path: String,
    device_nr: u32,
    format_set: bool,
    width: u32,
    height: u32,
    bytesperline: u32,
}

/// Lê o nome do card (campo `card[32]` do QUERYCAP, offset 16..48)
pub fn query_card(file: &File) -> Result<String, String> {
    let mut cap = [0u8; 104];
    let ret = unsafe { libc_ioctl(file.as_raw_fd(), VIDIOC_QUERYCAP, cap.as_mut_ptr().cast()) };
    if ret != 0 {
        return Err("ioctl QUERYCAP falhou".into());
    }
    let end = cap[16..48].iter().position(|b| *b == 0).unwrap_or(32);
    Ok(String::from_utf8_lossy(&cap[16..16 + end]).into_owned())
}

/// Verifica se o descritor pertence a um device v4l2loopback
pub fn is_loopback(file: &File) -> bool {
    let mut cap = [0u8; 104];
    let ret = unsafe { libc_ioctl(file.as_raw_fd(), VIDIOC_QUERYCAP, cap.as_mut_ptr().cast()) };
    if ret != 0 {
        return false;
    }
    let drv = String::from_utf8_lossy(&cap[0..16]);
    drv.starts_with("v4l2 loopback")
}

/// Verifica via sysfs se o dispositivo /dev/videoN está com buffers alocados ou leitor conectado
pub fn check_device_in_use(nr: u32) -> (bool, Option<String>) {
    let buf_path = format!("/sys/devices/virtual/video4linux/video{nr}/buffers");
    let fmt_path = format!("/sys/devices/virtual/video4linux/video{nr}/format");
    let buffers = std::fs::read_to_string(&buf_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let format = std::fs::read_to_string(&fmt_path)
        .ok()
        .map(|s| s.trim().to_string());
    (buffers > 0, format)
}

/// Lista todos os dispositivos /dev/videoN do tipo v4l2loopback com detalhes de uso
pub fn list_loopback_devices_detailed() -> Vec<LoopbackDevInfo> {
    let mut list = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev") else { return list };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if let Some(num_str) = name_str.strip_prefix("video")
            && let Ok(n) = num_str.parse::<u32>()
        {
            let path = format!("/dev/video{n}");
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(&path)
                && is_loopback(&file)
            {
                let card = query_card(&file).unwrap_or_else(|_| format!("video{n}"));
                let (in_use, format) = check_device_in_use(n);
                list.push(LoopbackDevInfo {
                    nr: n,
                    card,
                    in_use,
                    format,
                });
            }
        }
    }
    list.sort_by_key(|d| d.nr);
    list
}

/// Lista simplificada de loopback (número e nome)
pub fn list_loopback_devices() -> Vec<(u32, String)> {
    list_loopback_devices_detailed()
        .into_iter()
        .map(|d| (d.nr, d.card))
        .collect()
}

/// Adiciona dinamicamente um dispositivo via ioctl em /dev/v4l2loopback
pub fn add_loopback_ioctl(nr: i32, label: &str) -> Result<u32, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/v4l2loopback")
        .map_err(|e| format!("abrir /dev/v4l2loopback: {e}"))?;

    let mut conf = V4l2LoopbackConfig {
        output_nr: nr,
        unused: 0,
        card_label: [0; 32],
        min_width: 0,
        max_width: 0,
        min_height: 0,
        max_height: 0,
        max_buffers: 0,
        max_openers: 30,
        debug: 0,
        announce_all_caps: 0, // exclusive_caps=1
    };

    let label_bytes = label.as_bytes();
    let copy_len = label_bytes.len().min(31);
    conf.card_label[..copy_len].copy_from_slice(&label_bytes[..copy_len]);

    let ret = unsafe {
        libc_ioctl(
            file.as_raw_fd(),
            V4L2LOOPBACK_CTL_ADD,
            (&mut conf as *mut V4l2LoopbackConfig).cast(),
        )
    };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(17) && nr >= 0 {
            // EEXIST: dispositivo já existe
            return Ok(nr as u32);
        }
        return Err(format!("V4L2LOOPBACK_CTL_ADD falhou: {err}"));
    }

    Ok(ret as u32)
}

pub fn add_loopback_root_action(nr: i32, label: &str) -> Result<u32, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(tr("Administrator permission is required."));
    }
    if !std::path::Path::new("/dev/v4l2loopback").exists() {
        let device = (nr >= 0).then_some(nr).unwrap_or(9);
        let output = Command::new("/sbin/modprobe")
            .args(["v4l2loopback", "devices=1", "exclusive_caps=1", "max_openers=30"])
            .arg(format!("video_nr={device}"))
            .arg(format!("card_label={label}"))
            .env("LC_ALL", "C")
            .output()
            .map_err(|err| format!("modprobe: {err}"))?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            if error.contains("Module v4l2loopback not found") {
                return Err(tr("Install v4l2loopback for your running kernel, then enable the webcam again."));
            }
            return Err(trf("Could not load v4l2loopback. Check the kernel module, then try again. {error}",
                &[("error", error.trim().into())]));
        }
        wait_for_path("/dev/v4l2loopback");
        if File::open(format!("/dev/video{device}")).is_ok_and(|file| is_loopback(&file)) {
            return Ok(device as u32);
        }
    }
    add_loopback_ioctl(nr, label)
}

fn wait_for_device(nr: u32) {
    wait_for_path(&format!("/dev/video{nr}"));
}

fn wait_for_path(path: &str) {
    for _ in 0..30 {
        if std::path::Path::new(path).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Cria um novo dispositivo loopback v4l2loopback
pub fn create_loopback_device(preferred_nr: Option<u32>, label: &str) -> Result<u32, String> {
    let nr = preferred_nr.map(|n| n as i32).unwrap_or(-1);

    if let Ok(dev_nr) = add_loopback_ioctl(nr, label) {
        wait_for_device(dev_nr);
        return Ok(dev_nr);
    }

    let stopping = AUTHORIZATION.with(|auth| {
        auth.get().map(|(result, stopping)| result.clone().map(|()| Arc::clone(stopping)))
            .transpose()
    })?;
    let cancelled = || stopping.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire));
    if cancelled() {
        return Err(tr("Administrator authentication was cancelled. Enable the webcam again to retry."));
    }

    let current_exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut command = Command::new("/usr/bin/pkexec");
    if stopping.is_some() {
        command.arg("--disable-internal-agent");
    }
    let mut child = command.arg(current_exe)
        .arg("--add-loopback")
        .arg(nr.to_string())
        .arg(label)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("pkexec: {err}"))?;

    while stopping.is_some() && child.try_wait().map_err(|err| format!("pkexec: {err}"))?.is_none() {
        if cancelled() {
            let _ = child.kill();
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let output = child.wait_with_output().map_err(|err| format!("pkexec: {err}"))?;
    if cancelled() {
        return Err(tr("Administrator authentication was cancelled. Enable the webcam again to retry."));
    }

    match output.status.code() {
        Some(126) => return Err(tr("Administrator authentication was cancelled. Enable the webcam again to retry.")),
        Some(127) => {
            let error = String::from_utf8_lossy(&output.stderr);
            if !error.trim().is_empty() {
                crate::core::log::log("WARN", error.trim());
            }
            return Err(tr("Administrator authentication failed. Check your administrator account and try again."));
        }
        Some(0) => {}
        _ => {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let missing = "Install v4l2loopback for your running kernel, then enable the webcam again.";
            if error == missing {
                return Err(tr(missing));
            }
            if let Some(detail) = error.strip_prefix("Could not load v4l2loopback. Check the kernel module, then try again. ") {
                return Err(trf("Could not load v4l2loopback. Check the kernel module, then try again. {error}",
                    &[("error", detail.into())]));
            }
            return Err(trf("Could not set up the virtual webcam: {error}", &[("error", error)]));
        }
    }

    let dev_nr = String::from_utf8_lossy(&output.stdout).trim().parse()
        .map_err(|_| tr("The webcam setup returned an invalid device number."))?;
    wait_for_device(dev_nr);
    Ok(dev_nr)
}

/// Cria ou encontra um loopback livre que não esteja em `avoid`
pub fn create_or_find_free_loopback(
    avoid: &std::collections::HashSet<u32>,
    label: &str,
) -> Result<LoopbackWriter, String> {
    let existing = list_loopback_devices_detailed();
    // 1. Reutiliza loopback já criado e desocupado
    for dev in &existing {
        if !avoid.contains(&dev.nr) && !dev.in_use
            && let Ok(w) = LoopbackWriter::open(dev.nr)
        {
            crate::core::log::log(
                "INFO",
                &format!("reutilizando dispositivo loopback livre: /dev/video{}", dev.nr),
            );
            return Ok(w);
        }
    }

    // 2. Calcula próximo número livre (>= 9)
    let existing_nrs: std::collections::HashSet<u32> = existing.iter().map(|d| d.nr).collect();
    let mut next_nr = 9;
    while avoid.contains(&next_nr)
        || existing_nrs.contains(&next_nr)
        || std::path::Path::new(&format!("/dev/video{next_nr}")).exists()
    {
        next_nr += 1;
    }

    crate::core::log::log(
        "INFO",
        &format!("criando novo dispositivo loopback /dev/video{next_nr}..."),
    );
    let created_nr = create_loopback_device(Some(next_nr), label)?;
    LoopbackWriter::open(created_nr)
}

pub(super) fn open_requested_loopback(n: u32, label: &str) -> Result<LoopbackWriter, String> {
    let path = format!("/dev/video{n}");
    if !std::path::Path::new(&path).exists() {
        add_loopback_ioctl(n as i32, label).map_err(|error| {
            format!("{path} indisponível: {error}. Prepare o v4l2loopback antes de iniciar. Consulte lenvix --status.")
        })?;
        wait_for_device(n);
    }
    let writer = LoopbackWriter::open(n)?;
    if !is_loopback(&writer.file) {
        return Err(format!("{path} não é um dispositivo v4l2loopback."));
    }
    Ok(writer)
}

/// Tenta abrir um device loopback disponível:
/// 1. Tenta o device especificado `n` (/dev/videoN).
/// 2. Reutiliza outro loopback livre ou cria um novo dispositivo.
pub fn ensure_loopback(n: u32, label: &str) -> Result<LoopbackWriter, String> {
    if let Ok(w) = LoopbackWriter::open(n)
        && is_loopback(&w.file)
    {
        return Ok(w);
    }

    let available = list_loopback_devices_detailed();
    for dev in &available {
        if dev.nr != n && !dev.in_use
            && let Ok(w) = LoopbackWriter::open(dev.nr)
        {
            crate::core::log::log(
                "INFO",
                &format!("usando loopback alternativo disponível: /dev/video{}", dev.nr),
            );
            return Ok(w);
        }
    }

    if !std::path::Path::new(&format!("/dev/video{n}")).exists() {
        return LoopbackWriter::open(create_loopback_device(Some(n), label)?);
    }

    create_or_find_free_loopback(&std::collections::HashSet::new(), label).map_err(|error| {
        if available.iter().any(|device| device.nr == n && device.in_use) {
            let hint = trf("{device} is busy. Close the app using it or select another virtual device.",
                &[("device", format!("/dev/video{n}"))]);
            format!("{hint} {error}")
        } else {
            error
        }
    })
}

fn assert_ioctl_valid(file: &File, path: &str) -> Result<(), String> {
    // capability buffer (104 bytes): erro = device inválido
    let mut cap = [0u8; 104];
    let ret = unsafe { libc_ioctl(file.as_raw_fd(), VIDIOC_QUERYCAP, cap.as_mut_ptr().cast()) };
    if ret != 0 {
        return Err(format!("{path} não é um device V4L2 válido (ioctl QUERYCAP falhou)"));
    }
    Ok(())
}

impl LoopbackWriter {
    /// Abre um descritor de /dev/videoN
    pub fn open(dev_n: u32) -> Result<Self, String> {
        let path = format!("/dev/video{dev_n}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("abrir {path}: {e}"))?;
        assert_ioctl_valid(&file, &path)?;
        Ok(Self {
            file,
            path,
            device_nr: dev_n,
            format_set: false,
            width: 0,
            height: 0,
            bytesperline: 0,
        })
    }

    pub fn device_nr(&self) -> u32 {
        self.device_nr
    }

    /// Ajusta o formato (VIDIOC_S_FMT). Fiel ao webcamize: é chamado no 1.º frame.
    /// Se o tamanho MUDA com o writer aberto, o keep_format pode barrar (EBUSY):
    /// zeramos keep_format → S_FMT → religamos; e confirmamos com G_FMT.
    pub fn ensure_format(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.format_set && self.width == width && self.height == height {
            return Ok(());
        }
        // writer fresco: descobre o formato atual do device (G_FMT) — o
        // keep_format setado numa sessão anterior PERSISTE no device após o
        // writer fechar e congela o formato velho
        if self.width == 0 {
            self.query_format();
        }
        if !self.format_set && (self.width, self.height) == (width, height) {
            // device já está exatamente no formato desejado: adota sem S_FMT
            self.format_set = true;
            let _ = self.set_keep_format(1);
            return Ok(());
        }
        // soltar o lock de formato SEMPRE antes de reformatar (não só quando
        // format_set: o flag do kernel sobrevive ao writer anterior!)
        let was_set = self.format_set;
        let _ = self.set_keep_format(0);
        let mut fmt = V4l2Format {
            ty: V4L2_BUF_TYPE_VIDEO_OUTPUT,
            pix: V4l2PixFormat {
                width,
                height,
                pixelformat: V4L2_PIX_FMT_YUYV,
                field: V4L2_FIELD_NONE,
                bytesperline: width * 2,
                sizeimage: width * height * 2,
                colorspace: V4L2_COLORSPACE_DEFAULT,
                ..Default::default()
            },
            ..Default::default()
        };
        let ret =
            unsafe { libc_ioctl(self.file.as_raw_fd(), VIDIOC_S_FMT, &mut fmt as *mut V4l2Format as *mut _) };
        if ret != 0 {
            let io_err = std::io::Error::last_os_error();
            // restaurar o lock para o estado anterior
            if was_set {
                let _ = self.set_keep_format(1);
            }
            let extra = if io_err.raw_os_error() == Some(errno_invalid()) {
                " (formato congelado por keep_format/stale? recarregue o módulo v4l2loopback)"
            } else if io_err.raw_os_error() == Some(16) {
                " (EBUSY: observador segurando o device? desligue a webcam, escolha a saída e religue)"
            } else {
                ""
            };
            return Err(format!(
                "VIDIOC_S_FMT({width}x{height} YUYV) falhou no {}: errno={io_err}{extra}",
                self.path
            ));
        }
        // o driver REESCREEVE o struct com o formato real aplicado (pode clampar
        // silenciosamente — OBS com buffers alocados força clamp para o antigo!)
        let (gw, gh, bpl) = (fmt.pix.width, fmt.pix.height, fmt.pix.bytesperline);
        if (gw, gh) != (width, height) {
            self.width = gw;
            self.height = gh;
            self.bytesperline = if bpl == 0 { gw * 2 } else { bpl };
            let _ = self.set_keep_format(1);
            return Err(format!(
                "S_FMT: kernel recusou/clampou {width}x{height} → {gw}x{gh} \
                 (algum observador (ex.: OBS) segurando buffers? desabilite a fonte e tente de novo)"
            ));
        }
        self.format_set = true;
        self.width = gw;
        self.height = gh;
        self.bytesperline = if bpl == 0 { width * 2 } else { bpl };

        // congela nosso formato: S_FMT de observadores (probing OBS/Chromium) falha
        // e eles precisam usar G_FMT → YUYV (comportamento webcamizex)
        let _ = self.set_keep_format(1);

        Ok(())
    }

    /// w/h aceitos pelo kernel (para sanidade dos frames)
    pub fn dims(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Lê o formato atual do device (VIDIOC_G_FMT) sem mexer em nada —
    /// adota localmente (width/height/bytesperline) se for YUYV.
    fn query_format(&mut self) {
        let mut fmt = V4l2Format { ty: V4L2_BUF_TYPE_VIDEO_OUTPUT, ..Default::default() };
        let ret = unsafe {
            libc_ioctl(self.file.as_raw_fd(), VIDIOC_G_FMT, &mut fmt as *mut V4l2Format as *mut _)
        };
        if ret == 0 && fmt.pix.pixelformat == V4L2_PIX_FMT_YUYV {
            self.width = fmt.pix.width;
            self.height = fmt.pix.height;
            self.bytesperline = fmt.pix.bytesperline;
        }
    }

    fn set_keep_format(&self, on: i32) -> Result<(), String> {
        let mut ctrl = V4l2Control { id: V4L2_CTRL_ID_KEEP_FORMAT, value: on };
        let ret =
            unsafe { libc_ioctl(self.file.as_raw_fd(), VIDIOC_S_CTRL, &mut ctrl as *mut V4l2Control as *mut _) };
        if ret != 0 {
            return Err("VIDIOC_S_CTRL(keep_format) falhou".into());
        }
        Ok(())
    }

    /// Escreve um frame YUYV (write() direto no fd — modo loopback streaming inline).
    /// Len errado = frame destorcido/reador lê lixo → recusa em vez de escrever.
    pub fn write_frame(&mut self, yuyv: &[u8]) -> Result<(), String> {
        let expected = self.bytesperline as usize * self.height as usize;
        if expected != 0 && yuyv.len() != expected {
            return Err(format!(
                "frame {} bytes != esperado {expected} (device {}×{}, bpl {})",
                yuyv.len(),
                self.width,
                self.height,
                self.bytesperline
            ));
        }
        self.file
            .write_all(yuyv)
            .map_err(|e| format!("write() no {} falhou: {e}", self.path))
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Nome do card (para exibir "Lenvix (Canon EOS 4000D)" na GUI/tray)
    pub fn card_name(&self) -> String {
        query_card(&self.file).unwrap_or_else(|_| self.path.clone())
    }
}

// —— ioctl libc mínimo (sem dep nix)
unsafe extern "C" {
    fn ioctl(fd: i32, request: std::os::raw::c_ulong, ...) -> i32;
}

unsafe fn libc_ioctl(fd: i32, request: u32, arg: *mut std::ffi::c_void) -> i32 {
    unsafe { ioctl(fd, request as std::os::raw::c_ulong, arg) }
}
