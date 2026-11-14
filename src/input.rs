use egui::{Key, Modifiers};

use crate::model::ViewMode;

/// Everything the user can request with the keyboard. The UI does not read
/// keys directly: it reads `Action`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextImage,
    PrevImage,
    /// One row up in the grid.
    MoveUp,
    /// One row down in the grid.
    MoveDown,
    FirstImage,
    LastImage,
    /// Grid → viewer, opening the selected image.
    EnterViewer,
    /// Viewer → grid, keeping the position.
    BackToGrid,
    ToggleFullscreen,
    ZoomIn,
    ZoomOut,
    /// 100 % zoom: one image pixel per screen pixel.
    ZoomActual,
    /// Fit to window.
    ZoomFit,
    RotateLeft,
    RotateRight,
    OpenFolder,
    Quit,
}

/// In which view a shortcut makes sense.
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

/// The shortcut table, IrfanView style.
///
/// Shortcuts with a modifier come first: they are consumed before the
/// unmodified version of the same key (e.g. Cmd+0 before 0).
pub const BINDINGS: &[Binding] = &[
    // --- with modifier ---
    b(Modifiers::COMMAND, Key::O, Action::OpenFolder, Scope::Global),
    b(Modifiers::COMMAND, Key::Q, Action::Quit, Scope::Global),
    b(Modifiers::COMMAND, Key::Num0, Action::ZoomFit, Scope::ViewerOnly),
    // --- navigation ---
    b(Modifiers::NONE, Key::ArrowRight, Action::NextImage, Scope::Global),
    b(Modifiers::NONE, Key::ArrowLeft, Action::PrevImage, Scope::Global),
    b(Modifiers::NONE, Key::Space, Action::NextImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Backspace, Action::PrevImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::ArrowUp, Action::MoveUp, Scope::GridOnly),
    b(Modifiers::NONE, Key::ArrowDown, Action::MoveDown, Scope::GridOnly),
    // In the viewer, up and down also step: it is a single axis of traversal.
    b(Modifiers::NONE, Key::ArrowUp, Action::PrevImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::ArrowDown, Action::NextImage, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Home, Action::FirstImage, Scope::Global),
    b(Modifiers::NONE, Key::End, Action::LastImage, Scope::Global),
    // --- view switching ---
    b(Modifiers::NONE, Key::Enter, Action::EnterViewer, Scope::GridOnly),
    b(Modifiers::NONE, Key::Escape, Action::BackToGrid, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::F, Action::ToggleFullscreen, Scope::Global),
    // --- zoom ---
    b(Modifiers::NONE, Key::Plus, Action::ZoomIn, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Equals, Action::ZoomIn, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Minus, Action::ZoomOut, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::Num0, Action::ZoomActual, Scope::ViewerOnly),
    // --- rotation (view only, the file is untouched) ---
    b(Modifiers::NONE, Key::L, Action::RotateLeft, Scope::ViewerOnly),
    b(Modifiers::NONE, Key::R, Action::RotateRight, Scope::ViewerOnly),
];

/// Consumes from the `InputState` the keys that match the current mode and
/// returns the resulting actions.
///
/// Called **before** drawing the widgets, so the arrow keys are not swallowed
/// by the grid's `ScrollArea` or the thumbnail strip.
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
    fn no_ambiguous_shortcuts_in_the_same_mode() {
        for mode in [ViewMode::Grid, ViewMode::Viewer] {
            let active: Vec<_> = BINDINGS.iter().filter(|x| x.scope.applies_to(mode)).collect();
            for (i, a) in active.iter().enumerate() {
                for other in &active[i + 1..] {
                    assert!(
                        !(a.key == other.key && a.mods == other.mods),
                        "{:?}+{:?} is mapped twice in {mode:?}: {:?} and {:?}",
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
    fn arrows_navigate_in_both_modes() {
        for mode in [ViewMode::Grid, ViewMode::Viewer] {
            for (key, expected) in
                [(Key::ArrowRight, Action::NextImage), (Key::ArrowLeft, Action::PrevImage)]
            {
                let found = BINDINGS
                    .iter()
                    .find(|x| x.key == key && x.scope.applies_to(mode))
                    .map(|x| x.action);
                assert_eq!(found, Some(expected), "missing {key:?} in {mode:?}");
            }
        }
    }

    #[test]
    fn modified_shortcuts_come_before_unmodified_ones_of_the_same_key() {
        for (i, binding) in BINDINGS.iter().enumerate() {
            if binding.mods == Modifiers::NONE {
                continue;
            }
            let plain =
                BINDINGS.iter().position(|x| x.key == binding.key && x.mods == Modifiers::NONE);
            if let Some(plain) = plain {
                assert!(
                    i < plain,
                    "{:?} should be consumed before its unmodified version",
                    binding.key
                );
            }
        }
    }
}
