# Lenvix — Planejamento (fusão webcamize + Entangle)

> Estudo de engenharia reversa + especificação de produto para um aplicativo Rust,
> GUI dedicada a câmeras DSLR/mirrorless: **preview ao vivo**, **controle de configurações
> da câmera**, **webcam virtual compartilhável (OBS etc.)**, **foto e vídeo**, **execução
> em segundo plano com ícone na tray**.
>
> Status: planejamento (sem código). Base das observações: análise de binários instalados
> na máquina (REA + Ghidra 12.1.2) e extração de API via introspection do Flatpak do Entangle.

---

## 1. Metodologia e evidências utilizadas

| Alvo | Artefato | Método | Resultado |
|---|---|---|---|
| webcamize 2.0.1 | `/usr/sbin/webcamize` (ELF x86-64, 44 KB, stripped, SHA-256 `2248dc3ce34dabeb30e37ab44d5ea0625c8fbca2d90f5abd1ca8b1207ab063e8`) | REA `binary_overview` (Ghidra) + `decompile` (main @ `0x3640`, conversor @ `0x5690`) + inventário de símbolos dinâmicos/strings | Pipeline completo reconstruído (seção 2.1) |
| Entangle 3.0 | Flatpak `org.entangle_photo.Manager` (`files/bin/entangle`, `files/lib/libentangle_backend.so.0.0.0` 219 KB, stripped) | REA overview + `objdump -T` (imports gphoto2) + GIR (`share/gir-1.0/Entangle-0.1.gir`) — API pública completa | Mapeamento de classes/métodos da Camera (seção 2.2) |

Fatores-chave observados (não inferidos):

- **webcamize NÃO chama o binário ffmpeg**: linka `libgphoto2.so.6`, `libavformat.so.63`,
  `libavcodec.so.63`, `libswscale.so.10`, `libavutil.so.61`, `libkmod.so.2` diretamente.
- **Entangle é um backend GObject** (gphoto2 + libraw + gexiv2 + lcms2 + gudev + gstreamer)
  com o frontend GTK3 exposto via introspection GIR (`Entangle-0.1`).

---

## 2. Comportamento observado dos aplicativos de origem

### 2.1 webcamize 2.0.1 (W. Turner Abney, BSD-2-Clause)

**CLI observada (flags `getopt_long`):**

```
-c, --camera NAME       selecionar câmera por nome (autodetect por padrão)
-d, --device NUMBER     número do /dev/videoN do loopback
-p, --fps VALUE         limite de quadros por segundo (padrão: 60)
-l, --log-level LEVEL   DEBUG, INFO, WARN, FATAL (padrão INFO)
-b, --no-v4l2loopback   desativar carga/configuração do módulo v4l2loopback
-x, --no-convert        não converter o formato de entrada antes de escrever
-s, --status            imprimir relatório de status e sair
-H/--help, -v/--version
```

**Pipeline reconstruído a partir de `main` (decompilação) e imports:**

1. **Detecção de câmera (libgphoto2):**
   `gp_context_new` → `gp_camera_new` → `gp_camera_autodetect` (lista câmeras/portas USB).
   Se `--camera` dado: `gp_list_find_by_name`; se não encontrado, *fallback* para a
   primeira câmera (mensagem "not found, using first detected camera").
   Em seguida `gp_abilities_list_new/load/lookup_model/get_abilities` →
   `gp_camera_set_abilities`; `gp_port_info_list_new/load/count/lookup_path/get_info` →
   `gp_camera_set_port_info` → `gp_camera_init`.

2. **Webcam virtual (v4l2loopback):**
   - Verifica `/dev/v4l2loopback` (dispositivo de controle) — **via libkmod**:
     `kmod_new`, `kmod_module_new_from_name`, `kmod_module_probe_insert_module`
     ("The v4l2loopback module is present").
   - Fallback privilegiado: em falha de permissão, faz **fork + `execvp("sudo", argv)`**
     de si mesmo ("Webcamize requires sudo when using v4l2loopback!").
   - Opções do módulo observadas na string: **`devices=0 exclusive_caps=1`**
     (exclusive_caps faz o loopback expor apenas "capture", necessário p/ Zoom/Chromium).
   - Abre `/dev/video%d` com `open()`, valida com `ioctl` **VIDIOC_QUERYCAP**
     ("Device %s does not support video output", "is not a valid V4L2 device").
   - 1º frame define o formato: **VIDIOC_S_FMT** em **YUYV** ("V4L2 format set to WxH YUYV").
   - Ao sair: remove o dispositivo criado ("Failed to remove the webcam device /dev/video%d").

3. **Loop principal (thread única, com handler de sinal `SIGINT` para saída graciosa):**

   ```
   while (running):
     t0 = clock_gettime(CLOCK_MONOTONIC)
     gp_camera_capture_preview(camera, CameraFile)           // MJPEG do preview da câmera
     (ptr, size) = gp_file_get_data_and_size(...)
     se converter habilitado: converter MJPEG→YUYV(...)      // falha → escreve bytes crus
     se 1º frame ou formato pendente: setar VIDIOC_S_FMT no loopback
     write(fd, bytes_YUYV, size)
     dt = clock_gettime - t0
     se dt < (1e9 / fps): nanosleep(resto)
   ```

4. **Conversor MJPEG→YUYV (função @ `0x5690`):**
   - `av_malloc(tamanho)` + `avio_alloc_context` (stream **em memória**, sem arquivo);
   - `avformat_alloc_context` + `avformat_open_input(NULL)` → `avformat_find_stream_info`;
   - `avcodec_find_decoder` ("Found decoder: %s for format: %s" — MJPEG);
   - `avcodec_send_packet`/`avcodec_receive_frame`; há provisionamento para hwaccel
     (`av_hwdevice_ctx_create`, `av_hwdevice_find_type_by_name` — aceleração opcional);
   - `sws_getContext` + `sws_scale` para YUYV; `av_image_fill_arrays`/`av_image_get_buffer_size`;
   - Se qualquer etapa falha: **aviso e escreve os bytes MJPEG brutos no loopback**
     (V4L2 aceita alguns formatos crus — degradação graciosa).

**Requisitos funcionais "fiéis" do webcamize que o Lenvix herdará:** autodetect +
`-c`-equivalente, criação do loopback com `exclusive_caps`, formato YUYV derivado do
primeiro frame, rate-limit por FPS com *timing* monotônico, saída por `write()` no
`/dev/videoN`, fechamento limpo (remoção do device + `gp_camera_exit`), timestamps de
log com níveis, `--status` (relatório de saúde do app).

### 2.2 Entangle 3.0 (org.entangle_photo.Manager)

**Imports gphoto2 do backend (observados):** lista completa de gerenciamento igual ao
webcamize **mais** o uso de **configuração**: `gp_camera_get_config` / `gp_camera_set_config`
e navegação da árvore `CameraWidget` (`gp_widget_get_child_by_name`, `gp_widget_count_...`,
`gp_widget_get_choice`, etc.), além de eventos (`gp_camera_wait_for_event`), captura
(`gp_camera_capture`), arquivo (`gp_camera_file_get`, `gp_camera_file_delete`, mime, nome)
e resumo/manual (`gp_camera_get_summary`, `gp_camera_get_manual`, `gp_camera_get_about`).

**Classes públicas (GIR `Entangle-0.1`) e o que significam para o Lenvix:**

| Classe | Papel no Entangle (uso observável na API) | Tradução p/ Lenvix |
|---|---|---|
| `Camera` | open/close, `load_controls`/`save_controls`, `capture_image`, `preview_image`, **`set_viewfinder`** (start/stop do liveview), `autofocus`, `manualfocus`, `set_capture_target`, `set_clock`, download/delete de arquivos, `process_events` | Motor de câmera (cap. 4) |
| `CameraManager` | Janela principal que troca de câmera ativa | Janela principal (1 câmera por vez) |
| `Control*` (Panel, Range, Toggle, Choice, Text, Date, Group, Button) | Painel de expander que **renderiza a árvore de configs da câmera** widget-a-widget: Range→slider min/max/step, Toggle→switch, Choice→combobox, Group→sub-seções | Painel de configurações genérico dirigido pelos dados da câmera (não hard-coded) |
| `DeviceManager` (+ libgudev) | Hotplug USB de câmeras | Detecção automática + notificação na tray |
| `Session` | pasta de sessão + padrão de nome de arquivo + importação de mídia | Sessões de foto/vídeo (cap. 4, core-capture) |
| `ImageLoader`/`PixbufLoader`/`ThumbnailLoader` + **libraw** + **gexiv2** | Decodificação RAW (libraw), metadados EXIF (gexiv2), histograma (`ImageHistogram`) | Galeria/miniaturas + EXIF das capturas |
| `Media`/`Video` + gstreamer | exibição e download de vídeo da câmera | Gravação/download de vídeo |
| `ColourProfile`/`ColourProfileTransform` (lcms2) | gestão de cor ICC nos previews | Opcional (fase 5) |
| `Script*` (Peas plugins) | extensibilidade | Plugins Rust — fase opcional |
| Sinais `camera-file-captured`, `camera-controls-changed`, `camera-opened`, etc. | eventos assíncronos da câmera | Event bus interno |

**Requisitos funcionais "fiéis" do Entangle que o Lenvix herdará:** painel de controles
construído dinamicamente a partir de `gp_camera_get_config` (isso é *o* diferencial
vs. webcamize), eventos da câmera em thread própria, captura de imagem full-res com
download para sessão, e viewfinder (liveview) ligável/desligável pela API.

---

## 3. O que será o Lenvix (escopo do produto)

**Definição:** "uma GUI do webcamize com a alma do Entangle" — aplicativo desktop em Rust
onde o usuário conecta uma câmera (USB, gphoto2), vê um preview ao vivo, ajusta o que a
câmera permitir (abertura, ISO, WB, foco…), captura fotos e vídeos, e **simultaneamente**
expõe a câmera como **webcam virtual** para OBS/Zoom/Meet/Chromium. Continua rodando em
segundo plano (daemon + tray) mesmo com a janela fechada.

### 3.1 Requisitos funcionais (prioridade M=Must, S=Should, C=Could)

**Conexão e câmera**
- FR-01 (M) Detectar câmeras via libgphoto2 (autodetect + lista nome/porta).
- FR-02 (M) Escolher câmera ativa entre múltiplas; reconexão automática em hotplug
  (thread monitorando udev, + eventos "camera-opened/closed").
- FR-03 (M) Sessão de câmera: abrir/fechar (`gp_camera_init/exit`) corretamente ao
  trocar de câmera.
- FR-04 (S) Reportar capacidade detectada por câmera: captura, preview, settings,
  viewfinder (espelho das props `has-capture/preview/settings/viewfinder`).

**Preview ao vivo**
- FR-05 (M) Preview MJPEG contínuo (`gp_camera_capture_preview`) renderizado na GUI
  (~15–30 fps conforme câmera; *rate-limited* como o webcamize).
- FR-06 (M) FPS do preview configurável pelo usuário.
- FR-07 (S) Toggle "viewfinder" (start/stop liveview quando a câmera suportar, via
  `gp_camera_get_config` "viewfinder"/`set_viewfinder`).
- FR-08 (S) Zoom/digital do preview (pan/zoom local do lado da GUI).
- FR-09 (C) Gestão de cor ICC do preview (lcms2) e histograma.

**Configurações da câmera (diferencial Entangle)**
- FR-10 (M) Ler árvore `CameraWidget` (`gp_camera_get_config`) e construir painel UI
  dinâmico: Range→slider (min/max/step), Toggle→switch, Choice→combo, Text→campo,
  Date→datapicker, Group→sub-seções (fiel ao `ControlPanel`).
- FR-11 (M) Aplicar alterações (`gp_camera_set_config`) com confirmação/rollback em erro.
- FR-12 (S) Snaphots de configurações (salvar/reescrever perfis do usuário).
- FR-13 (S) Autofocus (trigger via `autofocus`) e manual focus com passos ±.
- FR-14 (S) Exibir apenas seções/widgets que a câmera expõe (política `has-settings`).

**Captura (foto/vídeo)**
- FR-15 (M) Capturar foto full-res (`gp_camera_capture`) com download para sessão
  (RAW+JPEG conforme câmera), com miniatura na galeria.
- FR-16 (M) Capturar a partir do preview (screen-grab do stream, útil p/ câmeras
  suportando apenas preview).
- FR-17 (S) **Gravação de vídeo**: duas estratégias sinalizadas na UI:
  (a) *câmeras com modo vídeo USB*: abrir stream de vídeo (ex.: "set capture target
  memory card"/`capture` movie file) e baixar ao final — fiel ao Entangle;
  (b) *modo webinar*: gravar o próprio preview MJPEG em MP4/MOV no PC
  (com anúncio explícito de resolução do preview, não da res. full da câmera).
  A limitação real de cada câmera será exibida no painel de gravação.
- FR-18 (S) Retomada automática do preview após captura (mesmo comportamento do
  Entangle: eventos resync do `wait_for_event`).
- FR-19 (C) Slot "capture target" (cartão/aux) e `set_clock` (sincronizar relógio da
  câmera com o do PC).

**Webcam virtual (diferencial webcamize)**
- FR-20 (M) Manter pipeline webcamize: loopback v4l2 → escreve YUYV decodificado do
  preview MJPEG em `/dev/videoN`, compartilhável por OBS/Zoom/Chromium.
- FR-21 (M) **`exclusive_caps=1`** por padrão (compat. Chromium/Zoom).
- FR-22 (M) Iniciar/parar webcam pela UI/tray; remover device ao fechar.
- FR-23 (M) Modo `--no-convert`-equivalente (escrever MJPEG cru quando converter falhar;
  e opção do usuário desativar conversão).
- FR-24 (S) Enquanto webcam ativa + observador: fallback para bytes brutos MJPEG quando
  erro de decodificação (degradação graciosa, como em webcamize).
- FR-25 (S) Indicador "webcam em uso" — detectar que o device está sendo lido por outro
  processo (consultar contadores/estado do loopback).

**Segundo plano / tray**
- FR-26 (M) Fechar a janela não mata o app: janela oculta na tray, streaming contínuo.
- FR-27 (M) Menu da tray: abrir/fechar janela, webcam on/off, última foto, sair (com
  limpeza do device).
- FR-28 (M) Notificações do sistema (captura concluída, erro de câmera, webcam ativa).
- FR-29 (S) Autostart opcional (desktop entry) e auto-iniciar webcam.
- FR-30 (C) Modo headless (serviço CLI) com socket de controle para a GUI? — decidir
  na arquitetura (cap. 4).

**Sessões e galeria**
- FR-31 (M) Pasta de sessão + padrão de nome de arquivo (espelho `Session`).
- FR-32 (S) Galeria das capturas com EXIF (gexiv2) e, p/ RAW, decodificação via `libraw`
  (bindings `rawler`/`libraw-sys` — fase 2).

### 3.2 Requisitos não funcionais

- **Robustez de limpeza**: todos os caminhos de saída (normal, crash, SIGINT/SIGTERM)
  devem remover o loopback e liberar a câmera (os dois "Cleanings up" do webcamize).
- **Permissões**: nem exigir app inteiro como root; dividir em ações privilegiadas
  mínimas (carregar `v4l2loopback`, criar `/dev/videoN`) — usar libkmod como webcamize
  e/ou udev rules/`polkit`; avisar e oferecer re-exec via `sudo` **só para o setup do
  módulo se a sessão sem permissão falhar** (fiel e aceitável).
- **Código**: workspace Cargo com 1 binário + módulos; sem `unsafe` além do necessário
  nos bindings/FDB.
- **Logs**: níveis DEBUG/INFO/WARN/FATAL em estilo syslog, com cores quando `isatty`.
- **Portabilidade-alvo**: Linux, Wayland + X11 (tray via StatusNotifierItem; fallback
  Xlib `(systray)`). Flatpak eventualmente (drivers gphoto2/udev blobs no runtime Sdk).

---

## 4. Arquitetura proposta (sem implementação)

### 4.1 Camadas

```
┌──────────────────────────────────────────────────────────────┐
│  GUI (janela principal, painel de controles, galeria, tray)  │
├──────────────────────────────────────────────────────────────┤
│  AppShell: gestão de sessões, configurações, notificações    │
├──────────────────────────────────────────────────────────────┤
│  core-camera        core-stream          core-capture        │
│  (gphoto2 wrapper)  (webcamize engine)   (foto/vídeo/sessão) │
├──────────────────────────────────────────────────────────────┤
│  bindings: libgphoto2-sys · libav (avcodec/scale) · kmod     │
└──────────────────────────────────────────────────────────────┘
```

- **core-camera**: thread dedicada que processa eventos (equivalente a
  `process_events`/`wait_for_event` do Entangle). API interna assíncrona (channels), com
  mensagens: `Detected`, `Opened`, `ControlsChanged(CameraWidget)`, `PreviewFrame(Vec<u8>)`,
  `FileCaptured`, `Closed`. Sem acesso direto da GUI ao handle do gphoto2.
- **core-stream**: engine estilo webcamize: decodifica o preview e escreve no loopback
  (um único decode alimenta a GUI e a webcam).
  - Dono único do `/dev/videoN` fd, formato YUYV derivado do 1.º frame (fiel webcamize),
    rate-limit por FPS monotônico.
  - Decodificação via libav (sws_scale), como webcamize; fallback `--no-convert`
    (escrever MJPEG cru) em caso de falha de conversão.
- **core-capture**: sessões (pasta + padrão), download (`gp_camera_file_get`), EXIF
  (gexiv2), montagem de vídeo (estratégia (b) do FR-17: muxer reaproveitando o mesmo
  decodificador do core-stream + encoder hw-acelerado opcional).
- **GUI**: renderizar preview via textura/render target compartilhando o frame já
  decodificado.
  Painel de controles: **gerado dinamicamente da CameraWidget tree** (mesma estratégia
  dos `Control*` do Entangle), com widgets GTK/Qt/egui correspondentes.
- **tray/daemon**: processo único com tray; fechar janela = ocultar;
  sair via tray com limpeza graciosa.

### 4.2 Sistemas Linux essenciais

| Sistema | Uso | Origem da fidelidade |
|---|---|---|
| v4l2loopback (kernel module) | cria `/dev/videoN` virtual; opções `devices=N exclusive_caps=1` | webcamize |
| libkmod / modprobe | carregar módulo se ausente; detectar "module is present" | webcamize |
| V4L2 ioctls (`VIDIOC_QUERYCAP`, `VIDIOC_S_FMT`) | validar device; definir pixel_format | webcamize |
| libgphoto2 | câmera: detect/init/config/capture/preview/files | ambos |
| udev (libgudev) | hotplug da câmera | Entangle |
| gvfsd (session bus) | pastas XDG e sessões de mídia | Entangle (Flatpak policy) |

### 4.3 Crates Rust candidatas (avaliar na fase 3, nenhuma decidida aqui)

| Necessidade | Candidatas | Riscos |
|---|---|---|
| gphoto2 | `gphoto2` crate (completa, ativa) vs. `libgphoto2-sys` manual | cobertura de widgets; assíncronia |
| V4L2 loopback / writer | `ioctl` cru via `nix` + `v4l2r` | poucos exemplos p/ "webcam writer" |
| Decodificação MJPEG | `ffmpeg-next` (avcodec direto, fiel ao webcamize) vs `turbojpeg` | licença/build do ffmpeg |
| Escala/pixfmt | `libswscale` via `ffmpeg-next`, ou `fast_image_resize` + conversão manual p/ YUYV | perf/compat |
| Kernel module | bindings de `libkmod` ou shell `modprobe` | poucos bindings mantidos |
| GUI | GTK4-rs (fiel GNOME/Entangle) · Tauri (webview) · egui/slint (compactas) · iced | integração com tray |
| Tray/background | `tray-icon` / dbus `StatusNotifierItem` (dependente do DE) | Wayland |
| Vídeo/gravar | `ffmpeg-next` (muxer) · `gstreamer-rs` | footprint |
| EXIF/RAW | gexiv2 bindings · `kamadak-exif` · RAW: `rawler` (Rust puro) | suporte RAW parcial |
| udev/plug | `tokio-udev` / `udev` bindings | maturidade |

> Nota: escolhas serão fechadas na fase de implementação + PoC com câmera física real
> (a câmera específica decide muita coisa: suporte a viewfinder/config no driver gphoto2).

---

## 5. Fases do roadmap

**Fase 0 — PoC de câmera (1 semana)**
- Ligação manual em Rust à libgphoto2: abrir a câmera do usuário, trocar uma config,
  capturar 1 preview. Valida premissas básicas: crate gphoto2, ciclo init/preview/config.

**Fase 1 — CLI-fiel do webcamize (1–2 semanas)**
- Stream engine + módulo kmod + loopback + YUYV + rate-limit + saída limpa.
  Binário headless `lenvix --device 9 --fps 30` observável por apps de webcam (OBS etc.).

**Fase 2 — GUI mínima (2–3 semanas)**
- Janela com preview renderizado do mesmo decode do core-stream; escolha de câmera;
  iniciar/parar webcam; tray/segundo plano.

**Fase 3 — Painel de configurações + captura (2–3 semanas)**
- Painel dinâmico da CameraWidget tree (fiel ao Entangle); foco; captura de imagem +
  download e sessão; galeria básica.

**Fase 4 — Vídeo e refinamentos (Executada na v0.2.0)**
- Estratégia de gravação (b) em MP4 (H.264 ultrafast) com timer dinâmico, fallback local e galeria integrada;
- Notificações de desktop nativas (D-Bus/notify-send) para foto capturada e vídeo gravado;
- Diagnóstico CLI `--status` detalhado (configurações, devices loopback, câmeras e sessão);
- Flags adicionais `--help` e `--version`.

**Fase 5 (opcional) — extras Entangle**
- Histograma, gestão de cor ICC, perfis de configuração salvos, plugins.

**Fase 6 — Suporte Multiplataforma: Windows (Última Fase)**
- Abstração do backend de webcam virtual: DirectShow Filter / Media Foundation Virtual Camera (driver OBS VirtualCam ou AkVirtualCamera) via `#[cfg(target_os = "windows")]`.
- Abstração de System Tray: migração de `ksni` (D-Bus/Linux) para abstração agnóstica (`tray-icon` Win32).
- Backend de câmera: suporte a WinUSB/libusb ou Canon EDSDK.
- Empacotamento e distribuição Windows: instalador executável (`.exe` via Inno Setup/NSIS) e versão portátil (`.zip`).
- CI/CD no GitHub Actions com runner `windows-latest`.

---

## 6. Riscos e limitações conhecidas (observados, não especulados)

1. **gphoto2 e câmeras**: suporte a "liveview" varia por driver/câmera;
   algumas câmeras expõem preview mas não controles (e vice-versa) — a UI deve mostrar
   apenas capacidades detectáveis (`has-preview/-settings/-viewfinder`).
2. **Exclusividade do gphoto2**: apenas um processo pode manter a câmera aberta por vez;
   um segundo app gphoto2 conectado simultaneamente costuma retornar o erro
   "camera is busy".
3. **v4l2loopback + sudo**: exigir root apenas na carga do módulo; documentar setup
   feito uma vez com udev rules, como caminho para "sem sudo no dia a dia".
4. **Compat. Chromium**: sem `exclusive_caps=1` alguns apps não "veem" o device
   (webcamize adota `exclusive_caps=1` — adota-se igual aqui).
5. **FPS real**: câmeras DSLR via `capture_preview` entregam aproximadamente 10–30 fps
   variável; o FPS configurado é um **teto**, não garantia.
6. **Gravação de vídeo full-res via USB não é genérica** — limitada a alguns drivers
   gphoto2. Por isso o FR-17 prevê duas estratégias.

---

## 7. Rastreabilidade (apêndice técnico)

### 7.1 webcamize — símbolos dinâmicos relevantes (extrato):

```
gp_abilities_list_{new,load,lookup_model,get_abilities}   gp_camera_{new,init,exit,free}
gp_camera_autodetect  gp_camera_capture_preview  gp_camera_set_{abilities,port_info}
gp_port_info_list_{new,load,count,lookup_path,get_info}
gp_file_new  gp_file_get_data_and_size  gp_list_*  gp_context_{new,_unref}
gp_result_as_string  gp_library_version
avformat_{alloc_context,open_input,find_stream_info,free_context,close_input}
avcodec_{alloc_context3,open2,find_decoder,send_packet,receive_frame,free_context}
sws_{getContext,scale,freeContext}  av_image_{get_buffer_size,fill_arrays}
avio_alloc_context  av_hwdevice_{ctx_create,find_type_by_name}
kmod_new  kmod_module_{new_from_name,probe_insert_module,unref}  kmod_unref
```

### 7.2 webcamize — strings-chave:

```
Usage: webcamize [OPTIONS...]   Webcamize 2.0.1, copyright (c) W. Turner Abney 2025
devices=0 exclusive_caps=1     /dev/v4l2loopback     /dev/video%d
V4L2 format set to %dx%d YUYV / Found decoder: %s for format: %s
The v4l2loopback module is present / Failed to insert v4l2loopback module: %s
Failed to remove the webcam device /dev/video%d / Webcamize requires sudo when using v4l2loopback!
Failed to convert image to YUYV, using original image data instead
```

### 7.3 Entangle — classes GIR relevantes:

`Camera, CameraManager, CameraAutomata, CameraFile, CameraList, CameraSupport,
CameraPreferences, ControlPanel, Control{Range,Toggle,Choice,Text,Date,Group,Button},
DeviceManager, Session, SessionBrowser, Media, Video, Image, ImageLoader,
PixbufLoader, ThumbnailLoader, ImageHistogram, ColourProfile{,Transform},
Script{,Config,Simple}, Preferences{,Display}, MediaPopup, MediaStatusbar, HelpAbout`.

`Camera` methods relevantes: `open/close{,_async}`, `load_controls`, `save_controls`,
`capture_image`, `preview_image`, `set_viewfinder`, `set_capture_target`, `set_clock`,
`autofocus`, `manualfocus`, `download_file`, `delete_file`, `process_events`,
`get_controls`, `get_summary`, `get_manual`.

### 7.4 Identidade dos artefatos

- webcamize: SHA-256 `2248dc3ce34dabeb30e37ab44d5ea0625c8fbca2d90f5abd1ca8b1207ab063e8`
  (NEEDED: libgphoto2.so.6, libgphoto2_port.so.12, libavformat.so.63, libavcodec.so.63,
  libswscale.so.10, libavutil.so.61, libkmod.so.2).
- Entangle Flatpak: `org.entangle_photo.Manager` 3.0 (GNOME runtime 49);
  backend NEEDED: libglib/gio/gobject, libgdk_pixbuf, libgstbase-1.0, libgphoto2.so.6,
  libgphoto2_port.so.12, liblcms2, libgudev-1.0, libgexiv2, libraw_r.

---

## 8. Pendências para a primeira sessão de código

1. Confirmar contra câmeras físicas disponíveis: preview, config tree, viewfinder.
2. Escolher GUI framework (recomendação pré-PoC: GTK4-rs, caminho GNOME/tray mais direto).
3. Decidir licença do release binário: build do avcodec GPL vs. LGPL.
4. Decidir canal de atualização/empacotamento (AUR/Flatpak).

---

## 9. Resultados da Fase 0 (PoC, executada)

**Ambiente:** Arch Linux, Rust 1.98.1, libgphoto2 2.5.34 (pkg-config), FFmpeg 9.0.1
(libavcodec 63 / libswscale 10.1.101 / libavutil 61 — mesmos majors linkados pelo webcamize),
libkmod 34.2, v4l2loopback 0.15.4 (DKMS) disponível (módulo *não* carregado por padrão,
não há `/dev/video*`), OBS instalado para testes. Câmera física: **Canon EOS 4000D**
(Rebel T100), USB 04a9:32d9, lente EF-S 18-55mm.

**Código:** `src/bin/poc_camera.rs` (binário PoC; `--set key=value` e `--probe N`).
Crate escolhida: **`gphoto2` 3.4.1** (API `Task<T>` com `.wait()`/async, widgets tipados
`Widget::{Group,Text,Range,Toggle,Radio,Button,Date}`).

**Validado contra a câmera real:**

| Premissa | Resultado |
|---|---|
| Autodetect + init (`fr-01/03`) | ✅ mesmo com gvfs-gphoto2 instalado (sem conflito de sessão) |
| `capture_preview` (FR-05) | ✅ MJPEG 1056×704 ~270 KB/frame, image/jpeg |
| Taxa real de preview | ⚠ ~6 fps (debug, sequencial) — confirmou o risco 5: fps é teto variável |
| Árvore de config (FR-10) | ✅ 5 seções (actions/settings/status/imgsettings/capturesettings/other), 60+ widgets |
| `gp_camera_set_config` (FR-11) | ✅ `reviewtime` alterado e persistido (revertido depois) |
| Abilities/capacidades (FR-04) | ✅ `CameraOperations`: capture_image ✓, capture_preview ✓, configure ✓, trigger_capture ✓, **capture_video ✗** |
| Viewfinder (FR-07) | ✅ widget `viewfinder` (Toggle) presente; `liveviewsize` (Large/Medium/Small); `eoszoom`/`eoszoomposition` (FR-08 viável) |
| Foco (FR-13) | ✅ `autofocusdrive` (Toggle), `manualfocusdrive` (Radio: Near/Far 1–3), `focusmode` (One Shot/AI Focus/AI Servo), `afmethod` (Live/LiveFace/Quick) |
| Vídeo (FR-17) | ⚠ `capture_video` **false** no driver; porém existem `eosmoviemode` (Toggle), `eosmovieswitch` (Text), `movierecordtarget`="SDRAM" — estratégia (b) (gravar o preview) é a via real para esta câmera |
| Exposição | ✅ `aperture` (3.5–22), `shutterspeed` ("bulb"–1/4000), `iso` (Auto/200–6400), todos Radio |
| Extras observados | `eosremoterelease` (Radio: Press Half/Full AF/MF, Release — obturador remoto **sem** download), `imageformat` RAW/RAW+L, `capturetarget` (Internal RAM/Memory card), `syncdatetime` (FR-19: set_clock via Toggle dedicado), bateria/lensname/shuttercounter status readonly |

**Decisões fechadas pela PoC:**

1. **gphoto2 crate (3.4.1)** aprovada — cobertura de widgets suficiente para o painel
   dinâmico do Entangle; `Task<T>` permite a thread de eventos do core-camera.
2. **FR-17(b)** confirmado como via primária de vídeo para a 4000D (sem captura de vídeo
   nativa pelo driver; gravar o preview MJPEG).
3. FPS do preview: configurar valor **teto** na UI (UI deve exibir o fps medido real).

---

## 10. Resultados da Fase 2 (GUI mínima + motor de webcam, executada)

**Estrutura de código (cargo workspace-de-fato):**

```
src/
├── main.rs               entry: tray + engine + GTK app
├── lib.rs                módulos da crate lenvix
├── core/
│   ├── camera.rs         engine: thread gphoto2 autônoma (comandos/eventos por channel),
│   │                     preview contínuo rate-limited, reabre câmera em erro
│   ├── stream.rs         LoopbackWriter: v4l2loopback (S_FMT YUYV, write()), pkexec modprobe
│   └── yuv.rs            RGB→RGBA (GTK) e RGB→YUYV (BT.601, puro Rust)
├── gui/app.rs            GTK4-rs: janela, preview via gdk::MemoryTexture (zune-jpeg decode),
│   │                     toggle webcam, fps-alvo, tray-integration (FR-05/06/20/22/26/27)
│   └── (UiAction channel tray→GTK)
├── tray.rs               ksni (SNI): Mostrar janela / Webcam on-off / Sair
└── bin/smoke.rs          smoke test headless da engine
```

**Decisões fechadas nesta fase:**

4. **GUI: GTK4-rs 0.11** (o caminho GNOME/tray direto recomendado em §8.2). Preview via
   `gdk::MemoryTexture::newался(... R8g8b8a8)`.
5. **Decoder MJPEG: `zune-jpeg` (puro Rust)** em vez de libav para o MVP — liberação do
   build ffmpeg (§8/pendência 3); conversão YUYV manual (BT.601). libav volta apenas em
   fase de vídeo (fr-17(b), muxer/encoder) — e reavaliada lá.
6. **v4l2loopback**: carregamento sob demanda (`pkexec modprobe` com `video_nr=9
   exclusive_caps=1`), device fixo `/dev/video9` para não pegar webcams físicas.
   Com `exclusive_caps=1`, o device só aparece como webcam enquanto nosso fd está
   aberto — parar a webcam remove da lista dos apps automaticamente (FR-22).
7. **Rate/timing fiel**: S_FMT derivado do 1.º frame + `write()` no fd, como o webcamize.

**Medido (release build, Canon EOS 4000D, 12 s de smoke test):**

| Pipeline | Resultado |
|---|---|
| decode+GUI/flags (debug) | teste rápido: 35 frames/12 s (~3 fps — decode debug build, já resolvido) |
| release `--bin smoke` | **305 frames / 29,4 fps** @1056×704 (FR-05 ok) |
| GTK4 MemoryTexture | preview renderizado na janela (flip-dados direto, sem re-decode) |
| tray (KDE Wayland/SNI) | ícone na tray com menu (StatusNotifierItem nativo Plasma) |

(Pendente dentro da Fase 2: validação visual do usuário + teste OBS com `v4l2loopback`
carregado — precisa de pkexec/sudo no primeiro toggle.)

### 10.1 Correções da integração v4l2loopback (encontradas em teste real)

1. **ABI do kernel**: `sizeof(struct v4l2_format)` = **208** no kernel 7.2.12-xanmod
   (não 204 como em kernels 5.x) — S_FMT com struct curto retorna `ENOTTY`.
   - `offsetof(fmt.pix)` = 8 (union alinhada a 8 — layout Rust mantido igual);
   - `sizeof(v4l2_capability)` = 104 (confere o valor usado no QUERYCAP);
   - `IOC` calculados confere: `VIDIOC_S_FMT=0xc0d05605`
2. **Negociação de formato**: leitores (probing do OBS/ffmpeg) chamam S_FMT e podem
   sobrepor o formato do writer → solução: após o nosso S_FMT(YUYV), setar o controle
   de módulo **`keep_format=1`** (id `0x0098f900` via `VIDIOC_S_CTRL`) — S_FMT de
   observadores passa a falhar e eles usam `G_FMT` (nosso YUYV).
3. **Formato stale**: um S_FMT de terceiro feito quando *não há writer* (ex.: OBS
   probe) fica congelado no device → `S_FMT` do nosso writer falha com formato
   antigão. Reset do módulo (modprobe -r) resolve; código reporta a condição
   com errno EINVAL.
4. **Estado validado end-to-end** (`v4l2-ctl` após writer ativo):
   `Video Capture 1056×704 YUYV 30fps` + round-trip com malloc de terceiro
   (`ffmpeg -f v4l2 -i /dev/video9`) retornou PNG íntegra do preview da Canon.
   Com `exclusive_caps=1`, o device só aparece como *webcam* enquanto o Lenvix
   mantém o fd aberto (parar na GUI/remove da lista dos apps — FR-22).

### 10.2 Diagnóstico de cor e nome do device (iteração com usuário)

1. **Bug de cor (resolvido)**: `rgb_to_yuyv` dividia o numerador de U/V por 512 em
   vez de 256 (shift >>9) → chroma comprimido pela metade ⇒ cores "lavadas" (produto
   confirmado no OBS: preview vivo, webcam morta). Diagnóstico numérico:
   `src/bin/yuvdiag.rs` — frame fixo pelos dois caminhos (decode direto vs.
   RGB→YUYV→loopback→leitor ffmpeg), médias R/G/B:
   - antes: R −7.4 / G +2.1 / B +2.8
   - depois: **R −1.1 / G −0.4 / B −0.6** (arredondamento)
2. **Card label dinâmico (FR-27/FR-25 UX)**: `card_label` do módulo agora recebe
   `Lenvix (<modelo da câmera>)` — ex.: `Lenvix (Canon EOS 4000D)`, visível no
   dropdown do OBS. Implementação: `query_card()` via QUERYCAP (card[32] @16..48);
   se o card difere, melhor esforço de `pkexec modprobe -r && modprobe` (falha
   graciosamente se OBS segurar o device ou usuário cancelar).
3. **Nota OBS**: fonte só mostra frames com o Lenvix *escrevendo*; se o Lenvix
   para/fecha, o device some (comportamento exclusivo-caps correto).

### 10.3 Fase 3 — painel dinâmico + captura

Implementado (novo `core/config.rs` + extensões em `core/camera.rs` e `gui/app.rs`):

1. **Árvore de config dinâmica** (`core/config.rs`): `push_tree` lê
   `camera.config()` (GroupWidget) e empurra `EngineEv::ConfigTree(Vec<CfgNode>)`
   — snapshot serializável com `name` (path `/`-joined), `label`, `kind`,
   `readonly`, `info`, `value` (`Text/Range{min,step}/Toggle/Radio{options}/Date/Button`)
   e filhos recursivos. Canon 4000D: 85 widgets (6 na raiz).
2. **Comandos novos**: `GetConfig` (re-push), `SetConfig{name,value}` (mapa
   string→tipo: Text/Range/Toggle/Radio; depois `gp_camera_set_single_config`
   sem re-get da árvore toda) e `PressButton{name}` (chama o callback do
   ButtonWidget via `press(cam)` — usado para AF/mirror-lockup do painel).
   Após cada SetConfig a tree completa é re-empurrada (GUI re-render).
3. **Painel dinâmico GUI**: Section → `Frame` aninhado; Text → Entry;
   Range → SpinButton (digits/step derivados do widget); Toggle → Switch;
   Radio → ComboBoxText; Button → botão clicável. Read-only vira insensitive.
4. **Captura (FR-17/§5 Fase 3)**: `EngineCmd::Capture` → `gp_camera_capture`
   full-res + `fs().download_to()` para `$HOME/.local/share/lenvix/session`
   (evento `CaptureDone{path,error}`). **retry 3× com pausa**: Canon USB rejeita
   capture_image junto com capture_preview em andamento com "I/O in progress".
   Galeria da sessão: FlowBox horizontal com miniaturas (gtk::Image::pixel_size),
   dupla-clique→`xdg-open` no arquivo local salvo.
5. **Validação (smoke `--bin smoke -- config`)**: ✅ tree 85 widgets;
   set no-op (`actions/manualfocusdrive`) e press (`actions/autofocusdrive`)
   sem erro; captura full-res + download ok (22–23 MB CR2 Raw baixado para
   `$HOME/.local/share/lenvix/session`); preview segue 29.7 fps durante tudo.
6. **Aprendidos**:
   - Canon entra em deep-sleep (auto-off) e deixa de responder via PTP
     ("Timeout reading/writing the port") — a câmera precisa estar desperta;
     o claim do USB (`gp_camera_open`) ainda funciona, mas o PTP mudo;
     acordar (shutter/MENU) resolve sem resetar USB.
   - `gp_camera_get_single_config` só resolve widgets da raiz ⇒ SetConfig
     percorre a tree com `get_child_by_name` (path `/`-joined), aplica e usa
     `gp_camera_set_single_config` no widget obtido.
   - "get_camera" duplica evento CameraOpened (1.º logo ao abrir) — cosmético, GUI já tolera.

### 10.4 Fase 3b — transform (zoom/pan/saída), log de disco e estabilizações

1. **Log de disco** (`core/log.rs`, sem deps): `$XDG_STATE_HOME/lenvix/lenvix.log`
   (fallback `~/.local/state`), uma linha por evento com timestamp civil próprio
   (algoritmo civil_from_days): INFO/WARN/ERROR para config aplicada/falhada,
   press de botões, captura sob/ok/erro, webcam on/off/Card, preview lost, Fatal.
2. **Scroll do painel**: re-render da tree mantia posição vertical do
   ScrolledWindow (vadjustment clone → render → restore) — pendente de patch
   (hoje re-render reseta scroll; workaround para próxima iteração).
3. **Transform** (`core/transform.rs`): `Transform{zoom,pan_x,pan_y,out}` — crop
  _odd-even-safe + escala nearest (ponto fixo 16.16); preview e webcam refletem.
   Saída: nativa / 1280×720 / 1920×1080 / 960×540 (16:9 para o OBS sem letterbox).
4. **Estabilizações de saída**:
   - `ensure_format` agora: keep_format=0 → S_FMT → G_FMT (dims aceitas pelo
     kernel, `bytesperline` real) → keep_format=1;新课 `dims()`.
   - `write_frame` valida len esperado (`bytesperline×h`) — recusa len errado
     em vez de colocar frame destorcido no leitor (era o "OBS quebrado").
   - EBUSY em S_FMT na troca de saída: engine **reverte transf.out para as dims
     do device e mantém a webcam viva** (zoom/pan preservados) + WARN no log +
     status na GUI; troca de saída recomendada com webcam desligada.
   - crop_x/crop_y pares (alinhamento de pares YUYV).

### 10.5 Fase 4 — Vídeo, Notificações e Diagnósticos CLI (Executada na v0.2.0 / Correção v0.2.1)

1. **Gravação de Vídeo Local (FR-17b)**:
   - Módulo [`core/recorder.rs`]: Gravação contínua do stream de vídeo processado em `.mp4` (H.264 ultrafast via FFmpeg com `-movflags +faststart`) com fallback para arquivo bruto se o FFmpeg não estiver instalado.
   - Botão **"Gravar vídeo"** / **"⏹ Parar (MM:SS)"** no painel da GUI com estilo de gravação e contador de tempo decorrido.
   - Suporte a reprodução imediata com `xdg-open` e miniaturas identificadas com ícone de vídeo na galeria da sessão.
2. **Notificações do Sistema Operacional**:
   - Módulo [`core/notify.rs`]: Emissão assíncrona não bloqueante de notificações desktop via `notify-send` (D-Bus) para fotos capturadas e gravações de vídeo concluídas (com nome e tamanho do arquivo).
3. **Diagnóstico e Relatório CLI `--status` (FR-34)**:
   - Adicionada flag `--status` ao binário executável reportando: configurações persistidas, dispositivos `/dev/videoN` do `v4l2loopback` com status de ocupação, câmeras USB detectadas pela `libgphoto2` e resumo de arquivos da sessão.
   - Suporte a `--help` / `-h` e `--version` / `-v`.
4. **Correção de Estabilidade de Layout (v0.2.1)**:
   - Travamento de dimensões horizontais e quebra de texto/elipsização com `max_width_chars` e `WrapMode::WordChar` nos rótulos de status e nos painéis laterais de controle e configurações, eliminando expansões involuntárias e preservando o espaço do preview.

---

## 11. Especificação da Fase 6 — Porte Multiplataforma para Windows (Última Fase)

O Lenvix nasceu como uma aplicação Linux-first centrada em `v4l2loopback` e chamadas diretas de kernel `ioctl`. Para expandir o produto para o ecossistema Windows sem comprometer a estabilidade do Linux, a **Fase 6** fecha o ciclo de desenvolvimento do projeto com os seguintes pilares:

### 11.1 Abstração do Core Stream (Webcam Virtual)
- **Linux:** mantido pipeline nativo `v4l2loopback` em `/dev/videoN` com `exclusive_caps=1`.
- **Windows:** implementação de `LoopbackWriter` para Windows usando interface de memória compartilhada / DirectShow Filter (compatível com o driver OBS VirtualCam / AkVirtualCamera) ou driver Media Foundation Virtual Camera.

### 11.2 Abstração da Bandeja do Sistema (Tray)
- Migração da camada `tray.rs` de `ksni` (D-Bus) para uma camada agnóstica como `tray-icon`, que interage diretamente com a Win32 API (`Shell_NotifyIcon`) no Windows e StatusNotifierItem/AppIndicator no Linux.

### 11.3 Comunicação com a Câmera
- Suporte no Windows via driver WinUSB / libusb para `libgphoto2`, ou integração com Canon EDSDK / Sony Camera Remote SDK.

### 11.4 Empacotamento e Entrega Windows
- **Instalador:** Script Inno Setup ou NSIS gerando `Lenvix-Setup-vX.Y.Z.exe` com associação de ícones e inicialização opcional com o Windows.
- **Portátil:** Pacote `Lenvix-vX.Y.Z-windows-x86_64.zip` contendo o binário estático e assets para execução sem instalação.
- **CI/CD:** Job de build com runner `windows-latest` no `.github/workflows/release.yml` para publicação automática de assets Windows no GitHub Releases.
