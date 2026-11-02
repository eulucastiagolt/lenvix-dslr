//! Multi-línguas (estilo gettext — o mesmo fluxo de traduções do WordPress/Godot).
//!
//! • As strings no código estão em inglês (msgid) — padrão internacional para
//!   que qualquer contribuidor entenda o contexto.
//! • Traduções ficam em arquivos `i18n/<idioma>.po` (ex.: `i18n/pt_BR.po`),
//!   texto puro editável sem compilar nada. O template para novas traduções
//!   é `i18n/lenvix.pot`.
//! • Em runtime: `i18n::init(Some("pt_BR"))` (settings) ou auto pelo ambiente
//!   (LC_ALL/LC_MESSAGES/LANG). Arquivo ausente = inglês (msgid).
//! • Polemizativos: `tr("...")` para strings fixas e `trf("... {var} ...", &[])`
//!   para interpolação — os tradutores usam os placeholders `{var}` do msgid.

use std::path::PathBuf;
use std::sync::OnceLock;

static CATALOG: OnceLock<gettext::Catalog> = OnceLock::new();
static LANG: OnceLock<String> = OnceLock::new();

/// Diretórios onde procuramos arquivos `<idioma>.po`, em ordem de prioridade:
/// LENVIX_I18N_DIR > AppImage (APPDIR) > XDG data > junto ao executável > cwd > /usr.
pub fn search_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(v) = std::env::var_os("LENVIX_I18N_DIR") {
        roots.push(PathBuf::from(v));
    }
    if let Some(appdir) = std::env::var_os("APPDIR") {
        roots.push(PathBuf::from(appdir).join("usr/share/lenvix/i18n"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(xdg).join("lenvix/i18n"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/lenvix/i18n"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        roots.push(parent.join("../share/lenvix/i18n"));
        roots.push(parent.join("i18n"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("i18n"));
    }
    roots.push(PathBuf::from("i18n"));
    roots.push(PathBuf::from("/usr/share/lenvix/i18n"));
    roots
}

/// Local env → candidatos de idioma: "pt_BR.UTF-8" → ["pt_BR", "pt"].
fn lang_candidates(explicit: Option<&str>) -> Vec<String> {
    fn add(base_raw: &str, out: &mut Vec<String>) {
        let base = base_raw
            .split('.')
            .next()
            .unwrap_or(base_raw)
            .replace('-', "_")
            .trim()
            .to_string();
        if base.is_empty() || base.eq_ignore_ascii_case("C") || base.eq_ignore_ascii_case("POSIX") {
            return;
        }
        if !out.contains(&base) {
            out.push(base.clone());
        }
        if let Some((primary, _)) = base.split_once('_') {
            let primary = primary.to_string();
            if !out.contains(&primary) {
                out.push(primary);
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    if let Some(l) = explicit.filter(|l| !l.trim().is_empty()) {
        add(l.trim(), &mut out);
        return out;
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(key) {
            add(&v, &mut out);
            if !out.is_empty() {
                break;
            }
        }
    }
    out
}

/// Inicializa o catálogo de traduções. Deve ser chamado uma única vez no
/// startup (main.rs), antes de tray/GUI. Falha silenciosa = strings em inglês.
pub fn init(language_setting: Option<&str>) {
    let cands = lang_candidates(language_setting);
    let mut loaded: Option<(gettext::Catalog, String)> = None;
    'outer: for cand in &cands {
        for root in search_roots() {
            let path = root.join(format!("{cand}.po"));
            if path.is_file() {
                match std::fs::File::open(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|f| gettext::Catalog::parse(std::io::BufReader::new(f)).map_err(|e| e.to_string()))
                {
                    Ok(cat) => {
                        crate::core::log::log(
                            "INFO",
                            &format!("i18n: idioma '{cand}' carregado de {}", path.display()),
                        );
                        loaded = Some((cat, cand.clone()));
                        break 'outer;
                    }
                    Err(e) => crate::core::log::log(
                        "WARN",
                        &format!("i18n: catálogo {path:?} inválido: {e}"),
                    ),
                }
            }
        }
    }
    if loaded.is_none() {
        crate::core::log::log(
            "INFO",
            &format!("i18n: sem tradução para {:?} — usando strings em inglês", cands),
        );
    }
    let (cat, lang) = loaded.unwrap_or_else(|| (gettext::Catalog::empty(), "en".to_string()));
    let _ = LANG.set(lang);
    let _ = CATALOG.set(cat);
}

/// Idioma ativo ("" / "en" quando não há .po).
pub fn lang() -> &'static str {
    LANG.get().map(|s| s.as_str()).unwrap_or("en")
}

/// Traduz msgid (inglês). Sem catálogo: retorna o próprio msgid.
pub fn tr(msg: &str) -> String {
    match CATALOG.get() {
        Some(c) => c.gettext(msg).to_string(),
        None => msg.to_string(),
    }
}

/// Traduz msgid com placeholders `{var}` → valor.
pub fn trf(msg: &str, vars: &[(&str, String)]) -> String {
    let mut s = tr(msg);
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// Idiomas disponíveis nos roots (para o seletor na tela de configurações).
pub fn available() -> Vec<String> {
    let mut langs = std::collections::BTreeSet::new();
    for root in search_roots() {
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("po")
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                {
                    langs.insert(stem.to_string());
                }
            }
        }
    }
    langs.into_iter().collect()
}
