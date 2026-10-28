//! Snapshot serializável da árvore de CameraWidget (cópia fiel para a GUI).
//!
//! A engine lê a tree do gphoto2 na thread dela e empurra um [`ConfigSnapshot`]
//! para a GUI construir o painel dinâmico (estilo Entangle) sem tocar na câmera.

/// Valor/estado de um widget
#[derive(Debug, Clone)]
pub enum CfgValue {
    Text(String),
    Range { value: f32, min: f32, max: f32, step: f32 },
    Toggle(bool),
    Radio { options: Vec<String>, current: String },
    Date(i64),
    Button,
}

/// Tipo do widget (para layout)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfgKind {
    Section,
    Text,
    Range,
    Toggle,
    Radio,
    Button,
    Date,
}

/// Nó da árvore: name é o caminho usado no EngineCmd::SetConfig
#[derive(Debug, Clone)]
pub struct CfgNode {
    pub name: String,
    pub label: String,
    pub kind: CfgKind,
    pub readonly: bool,
    pub info: String,
    pub value: Option<CfgValue>,
    pub children: Vec<CfgNode>,
}

impl CfgNode {
    /// Só alteráveis (skip widgets read-only e o Section "actions" vazio)
    pub fn is_action(&self) -> bool {
        matches!(self.kind, CfgKind::Button)
    }
}
