//! What the recording panel's buttons and keys do, independent of drawing.

/// Normal controls, or the "discard this recording?" question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelMode {
    Normal,
    Confirming,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelInput {
    Discard,
    Confirm,
    Back,
    Escape,
    Enter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Cancel,
}

/// Discarding always asks first; only the confirmation cancels the recording.
pub fn panel_step(mode: PanelMode, input: PanelInput) -> (PanelMode, Option<PanelAction>) {
    use PanelInput::*;
    match (mode, input) {
        (PanelMode::Normal, Discard | Escape) => (PanelMode::Confirming, None),
        (PanelMode::Confirming, Confirm | Enter) => (PanelMode::Normal, Some(PanelAction::Cancel)),
        (PanelMode::Confirming, Back | Escape) => (PanelMode::Normal, None),
        (mode, _) => (mode, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_step_follows_the_table() {
        use PanelInput::*;
        use PanelMode::*;
        let cases = [
            (Normal, Discard, Confirming, None),
            (Normal, Escape, Confirming, None),
            (Normal, Enter, Normal, None),
            (Confirming, Confirm, Normal, Some(PanelAction::Cancel)),
            (Confirming, Enter, Normal, Some(PanelAction::Cancel)),
            (Confirming, Back, Normal, None),
            (Confirming, Escape, Normal, None),
        ];
        for (mode, input, next, action) in cases {
            assert_eq!(panel_step(mode, input), (next, action), "{mode:?} + {input:?}");
        }
    }
}
