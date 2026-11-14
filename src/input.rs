use egui::{Key, Modifiers};

use crate::model::ViewMode;

/// Todo lo que el usuario puede pedir con el teclado. La UI no lee teclas
/// directamente: lee `Action`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextImage,
    PrevImage,
    /// Una fila hacia arriba en la grilla.
    MoveUp,
    /// Una fila hacia abajo en la grilla.
    MoveDown,
    FirstImage,
    LastImage,
    /// Grilla → visor, abriendo la imagen seleccionada.
    EnterViewer,
    /// Visor → grilla, conservando la posición.
    BackToGrid,
    ToggleFullscreen,
    ZoomIn,
    ZoomOut,
    /// Zoom 100 %, un píxel de imagen por píxel de pantalla.
    ZoomActual,
    /// Ajustar a la ventana.
    ZoomFit,
    RotateLeft,
    RotateRight,
    OpenFolder,
    Quit,
}

/// En qué vista tiene sentido un atajo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    GridOnly,
    ViewerOnly,
}

impl Scope {
    fn applies_to(self, mode: ViewMode) -> bool {
        match self {
            Scope::Global => true,
            Scope::GridOnly => mode == ViewMode::Grid,
            Scope::ViewerOnly => mode == ViewMode::Viewer,
        }
    }
}

pub struct Binding {
    pub mods: Modifiers,
    pub key: Key,
    pub action: Action,
    pub scope: Scope,
}

const fn b(mods: Modifiers, key: Key, action: Action, scope: Scope) -> Binding {
    Binding { mods, key, action, scope }
}

/// La tabla de atajos, al estilo IrfanView.
///
/// Los atajos con modificador van primero: se consumen antes que su versión
/// sin modificador para la misma tecla (p. ej. Cmd+0 antes que 0).
pub const BINDINGS: &[Binding] = &[
    // --- con modificador ---
    b(Modifiers::COMMAND, Key::O, Action::OpenFolder, Scope::Global),
    b(Modifiers::COMMAND, Key::Q, Action::Quit, Scope::Global),
    b(Modifiers::COMMAND, Key::Num0, Action::ZoomFit, Scope::ViewerOnly),
    // --- navegación ---
    b(Modifiers::NONE, Key::ArrowRight, Action::NextImage, Scope::Global),
    b(Modifiers::NONE, Key::ArrowLeft, Action::PrevImage, Scope::Global),
    b(Modifiers::NONE, Key::Space, Action::NextImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Backspace, Action::PrevImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::ArrowUp, Action::MoveUp, Scope::GridOnly),
    b(Modifiers::NONE, Key::ArrowDown, Action::MoveDown, Scope::GridOnly),
    // En el visor, arriba y abajo también avanzan: es un solo eje de recorrido.
    b(Modifiers::NONE, Key::ArrowUp, Action::PrevImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::ArrowDown, Action::NextImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Home, Action::FirstImage, Scope::Global),
    b(Modifiers::NONE, Key::End, Action::LastImage, Scope::Global),
    // --- cambio de vista ---
    b(Modifiers::NONE, Key::Enter, Action::EnterViewer, Scope::GridOnly),
    b(Modifiers::NONE, Key::Escape, Action::BackToGrid, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::F, Action::ToggleFullscreen, Scope::Global),
    // --- zoom ---
    b(Modifiers::NONE, Key::Plus, Action::ZoomIn, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Equals, Action::ZoomIn, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Minus, Action::ZoomOut, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Num0, Action::ZoomActual, Scope::ViewerOnly),
    // --- rotación (solo la vista, no toca el archivo) ---
    b(Modifiers::NONE, Key::L, Action::RotateLeft, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::R, Action::RotateRight, Scope::ViewerOnly),
];

/// Consume del `InputState` las teclas que correspondan al modo actual y
/// devuelve las acciones resultantes.
///
/// Se llama **antes** de dibujar los widgets, para que las flechas no se las
/// queden el `ScrollArea` de la grilla ni la tira de miniaturas.
pub fn poll(input: &mut egui::InputState, mode: ViewMode) -> Vec<Action> {
    BINDINGS
        .iter()
        .filter(|binding| binding.scope.applies_to(mode))
        .filter(|binding| input.consume_key(binding.mods, binding.key))
        .map(|binding| binding.action)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_hay_atajos_ambiguos_en_un_mismo_modo() {
        for mode in [ViewMode::Grid, ViewMode::Viewer] {
            let active: Vec<_> = BINDINGS.iter().filter(|x| x.scope.applies_to(mode)).collect();
            for (i, a) in active.iter().enumerate() {
                for other in &active[i + 1..] {
                    assert!(
                        !(a.key == other.key && a.mods == other.mods),
                        "{:?}+{:?} está mapeada dos veces en {mode:?}: {:?} y {:?}",
                        a.mods,
                        a.key,
                        a.action,
                        other.action
                    );
                }
            }
        }
    }

    #[test]
    fn las_flechas_navegan_en_ambos_modos() {
        for mode in [ViewMode::Grid, ViewMode::Viewer] {
            for (key, expected) in
                [(Key::ArrowRight, Action::NextImage), (Key::ArrowLeft, Action::PrevImage)]
            {
                let found = BINDINGS
                    .iter()
                    .find(|x| x.key == key && x.scope.applies_to(mode))
                    .map(|x| x.action);
                assert_eq!(found, Some(expected), "falta {key:?} en {mode:?}");
            }
        }
    }

    #[test]
    fn los_atajos_con_modificador_van_antes_que_los_de_la_misma_tecla_sin_el() {
        for (i, binding) in BINDINGS.iter().enumerate() {
            if binding.mods == Modifiers::NONE {
                continue;
            }
            let plain =
                BINDINGS.iter().position(|x| x.key == binding.key && x.mods == Modifiers::NONE);
            if let Some(plain) = plain {
                assert!(
                    i < plain,
                    "{:?} debería consumirse antes que su versión sin modificador",
                    binding.key
                );
            }
        }
    }
}
