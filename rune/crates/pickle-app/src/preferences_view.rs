//! Reading preferences change request policy; appearance preferences keep the current request alive.
use crate::controller::AppState;
use pickle_core::ReadingLevel;
use rune::{Color, El, col, row, text, tok::TextStyle};

const INK: Color = Color {
    r: 0.78741,
    g: 0.89001,
    b: 0.72935,
    a: 1.0,
};
const MUTED: Color = Color {
    r: 0.35469,
    g: 0.47700,
    b: 0.38006,
    a: 1.0,
};

fn heading(label: &str) -> El {
    text(label).font(TextStyle::new(17.0, 600.0)).color(INK)
}
fn note(label: &str) -> El {
    text(label)
        .wrap()
        .w_full()
        .shrink()
        .font(TextStyle::new(12.0, 400.0).lh(18.0))
        .color(MUTED)
}

fn level_button(s: &AppState, key: &str, level: ReadingLevel) -> El {
    let selected = s.settings.level == level;
    crate::views::action(s, key, level.label(), selected, move |state, _| {
        if state.settings_pending {
            return;
        }
        if state.settings.level != level {
            state.cancel();
            state.settings.level = level;
            state.save_appearance();
        }
    })
    .role("radio")
    .checked(selected)
    .focus_selected(selected)
    .disabled(s.settings_pending)
    .grow(1.0)
}

fn clean_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        1.0
    }
}
fn adjust_unit(value: f32, step: f32) -> f32 {
    ((clean_unit(value) * 10.0 + step).round() / 10.0).clamp(0.0, 1.0)
}

fn stepper(
    s: &AppState,
    key: &str,
    title: &str,
    detail: &str,
    value: String,
    enabled: (bool, bool),
    adjust: fn(&mut AppState, bool),
) -> El {
    let lower_key = format!("{key}-lower");
    let higher_key = format!("{key}-higher");
    row()
        .key(key)
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(text(title).font(TextStyle::new(14.0, 500.0)).color(INK))
                .child(note(detail)),
        )
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    crate::views::action(s, &lower_key, "−", false, move |state, _| {
                        if state.settings_pending {
                            return;
                        }
                        adjust(state, false)
                    })
                    .label(format!("Decrease {}", title.to_lowercase()))
                    .disabled(s.settings_pending || !enabled.0),
                )
                .child(
                    row()
                        .key(format!("{key}-value"))
                        .w(54.0)
                        .h(34.0)
                        .center()
                        .child(text(value).font(TextStyle::new(13.0, 500.0)).color(INK)),
                )
                .child(
                    crate::views::action(s, &higher_key, "+", false, move |state, _| {
                        if state.settings_pending {
                            return;
                        }
                        adjust(state, true)
                    })
                    .label(format!("Increase {}", title.to_lowercase()))
                    .disabled(s.settings_pending || !enabled.1),
                ),
        )
}

fn change_font(state: &mut AppState, higher: bool) {
    let value = if state.settings.text_size.is_finite() {
        state.settings.text_size
    } else {
        18.0
    };
    state.settings.text_size = (value + if higher { 1.0 } else { -1.0 })
        .round()
        .clamp(14.0, 24.0);
    state.save_appearance();
}
fn change_opacity(state: &mut AppState, higher: bool) {
    state.settings.background_opacity = adjust_unit(
        state.settings.background_opacity,
        if higher { 1.0 } else { -1.0 },
    );
    state.save_appearance();
}
fn change_intensity(state: &mut AppState, higher: bool) {
    state.settings.theme_intensity = adjust_unit(
        state.settings.theme_intensity,
        if higher { 1.0 } else { -1.0 },
    );
    state.save_appearance();
}

pub fn reading_and_appearance(s: &AppState) -> El {
    let opacity = clean_unit(s.settings.background_opacity);
    let intensity = clean_unit(s.settings.theme_intensity);
    col().key("reading-and-appearance").w_full().gap(24.0)
        .child(col().w_full().gap(10.0)
            .child(heading("Reading"))
            .child(note("Choose how Pickle explains a passage. Changing this stops the current answer; it does not rewrite completed answers."))
            .child(col().key("reading-level-group").role("radiogroup").label("Explanation reading level").focus_group().w_full().gap(8.0)
                .child(row().w_full().gap(8.0)
                    .child(level_button(s,"reading-level-automatic",ReadingLevel::Automatic))
                    .child(level_button(s,"reading-level-plain",ReadingLevel::Plain)))
                .child(row().w_full().gap(8.0)
                    .child(level_button(s,"reading-level-balanced",ReadingLevel::Balanced))
                    .child(level_button(s,"reading-level-technical",ReadingLevel::Technical)))))
        .child(col().w_full().gap(16.0)
            .child(heading("Appearance"))
            .child(note("Changes preview immediately while your answer keeps running."))
            .child(stepper(s,"pref-font","Reading text size","Set the answer text between 14 and 24 points.",format!("{:.0} pt",s.settings.text_size),(s.settings.text_size>14.0,s.settings.text_size<24.0),change_font))
            .child(stepper(s,"pref-opacity","Background opacity","Lower values let more of the desktop show through.",format!("{:.0}%",opacity*100.0),(opacity>0.0,opacity<1.0),change_opacity))
            .child(stepper(s,"pref-intensity","Theme intensity","Adjust the strength of Pickle’s green accents.",format!("{:.0}%",intensity*100.0),(intensity>0.0,intensity<1.0),change_intensity)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::Activity;
    use rune::{Cx, Env, Runtime};
    use tokio_util::sync::CancellationToken;

    fn view(state: &AppState, _: &mut Cx<AppState>) -> El {
        col()
            .w_full()
            .h_full()
            .padding(20.0)
            .child(reading_and_appearance(state))
    }
    fn runtime() -> Runtime<AppState> {
        Runtime::new(
            AppState::default(),
            view,
            (560.0, 700.0),
            Env::new(true, 1.0),
        )
    }
    fn click(runtime: &mut Runtime<AppState>, key: &str) {
        runtime.frame(1.0 / 60.0);
        let (x, y) = runtime
            .find(key)
            .unwrap_or_else(|| panic!("Missing preferences control {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0 / 60.0);
        runtime.frame(1.0 / 60.0);
    }
    fn running(runtime: &mut Runtime<AppState>) -> CancellationToken {
        let cancel = CancellationToken::new();
        runtime.state.activity = Activity::Running {
            id: runtime.state.revision,
            cancel: cancel.clone(),
            progress: "Writing".into(),
        };
        runtime.state.draft = "An answer in progress".into();
        cancel
    }

    #[test]
    fn appearance_controls_do_not_cancel_or_clear_a_streaming_answer() {
        let mut runtime = runtime();
        let cancel = running(&mut runtime);
        let revision = runtime.state.revision;
        click(&mut runtime, "pref-opacity-lower");
        click(&mut runtime, "pref-intensity-lower");
        click(&mut runtime, "pref-font-higher");
        assert_eq!(runtime.state.settings.background_opacity, 0.9);
        assert_eq!(runtime.state.settings.theme_intensity, 0.9);
        assert_eq!(runtime.state.settings.text_size, 19.0);
        assert!(!cancel.is_cancelled());
        assert_eq!(runtime.state.revision, revision);
        assert_eq!(runtime.state.draft, "An answer in progress");
        assert!(matches!(runtime.state.activity, Activity::Running { .. }));
    }

    #[test]
    fn changing_reading_level_cancels_but_reselecting_current_level_does_not() {
        let mut runtime = runtime();
        let cancel = running(&mut runtime);
        click(&mut runtime, "reading-level-automatic");
        assert!(!cancel.is_cancelled());
        click(&mut runtime, "reading-level-plain");
        assert!(cancel.is_cancelled());
        assert_eq!(runtime.state.settings.level, ReadingLevel::Plain);
        assert!(matches!(runtime.state.activity, Activity::Idle));
        assert!(runtime.state.draft.is_empty());
        click(&mut runtime, "reading-level-balanced");
        assert_eq!(runtime.state.settings.level, ReadingLevel::Balanced);
        click(&mut runtime, "reading-level-technical");
        assert_eq!(runtime.state.settings.level, ReadingLevel::Technical);
    }

    #[test]
    fn controls_enforce_font_and_percentage_bounds() {
        let mut runtime = runtime();
        for _ in 0..20 {
            click(&mut runtime, "pref-font-higher");
            click(&mut runtime, "pref-opacity-lower");
            click(&mut runtime, "pref-intensity-lower");
        }
        assert_eq!(runtime.state.settings.text_size, 24.0);
        assert_eq!(runtime.state.settings.background_opacity, 0.0);
        assert_eq!(runtime.state.settings.theme_intensity, 0.0);
        for _ in 0..20 {
            click(&mut runtime, "pref-font-lower");
            click(&mut runtime, "pref-opacity-higher");
            click(&mut runtime, "pref-intensity-higher");
        }
        assert_eq!(runtime.state.settings.text_size, 14.0);
        assert_eq!(runtime.state.settings.background_opacity, 1.0);
        assert_eq!(runtime.state.settings.theme_intensity, 1.0);
    }

    #[test]
    fn pending_connection_save_blocks_preferences_without_cancelling_the_answer() {
        let mut runtime = runtime();
        let cancel = running(&mut runtime);
        runtime.state.settings_pending = true;
        for key in [
            "reading-level-plain",
            "reading-level-balanced",
            "reading-level-technical",
            "pref-font-lower",
            "pref-font-higher",
            "pref-opacity-lower",
            "pref-intensity-lower",
        ] {
            click(&mut runtime, key);
        }
        assert_eq!(runtime.state.settings.level, ReadingLevel::Automatic);
        assert_eq!(runtime.state.settings.text_size, 18.0);
        assert_eq!(runtime.state.settings.background_opacity, 1.0);
        assert_eq!(runtime.state.settings.theme_intensity, 1.0);
        assert!(!cancel.is_cancelled());
    }

    #[test]
    fn preference_controls_fit_the_minimum_reader_width() {
        for width in [480.0, 560.0, 900.0] {
            let mut runtime = Runtime::new(
                AppState::default(),
                view,
                (width, 700.0),
                Env::new(true, 2.0),
            );
            runtime.frame(0.0);
            for key in [
                "reading-level-automatic",
                "reading-level-plain",
                "reading-level-balanced",
                "reading-level-technical",
                "pref-font-lower",
                "pref-font-higher",
                "pref-opacity-lower",
                "pref-opacity-higher",
                "pref-intensity-lower",
                "pref-intensity-higher",
            ] {
                let (bounds, _) = runtime.presented(key).unwrap();
                assert!(
                    bounds[0] >= 0.0 && bounds[0] + bounds[2] <= width + 0.5,
                    "{key} overflows at {width}: {bounds:?}"
                );
                assert!(
                    bounds[1] >= 0.0 && bounds[1] + bounds[3] <= 700.5,
                    "{key} is clipped: {bounds:?}"
                );
            }
        }
    }

    #[test]
    fn keyboard_navigation_changes_the_radio_choice_and_reaches_text_controls() {
        use rune::{Key, KeyEvent, Mods};
        let mut runtime = runtime();
        runtime.set_full_keyboard_access(true);
        runtime.frame(0.0);
        for key in [Key::Tab, Key::Right] {
            runtime.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
            runtime.frame(0.0);
        }
        assert_eq!(runtime.state.settings.level, ReadingLevel::Plain);
        runtime.key(KeyEvent {
            key: Key::Tab,
            mods: Mods::default(),
        });
        runtime.frame(0.0);
        runtime.key(KeyEvent {
            key: Key::Text(" ".into()),
            mods: Mods::default(),
        });
        runtime.frame(0.0);
        assert_eq!(runtime.state.settings.text_size, 17.0);
    }

    fn node<'a>(tree: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        if tree["key"] == key {
            return Some(tree);
        }
        tree["children"]
            .as_array()?
            .iter()
            .find_map(|child| node(child, key))
    }

    #[test]
    fn accessibility_tree_names_controls_and_reports_selected_reading_level() {
        let mut runtime = runtime();
        runtime.frame(0.0);
        let tree = runtime.accessibility_json();
        let selected = node(&tree, "reading-level-automatic").unwrap();
        assert_eq!(selected["role"], "radio-button");
        assert!(
            selected["states"]
                .as_array()
                .unwrap()
                .iter()
                .any(|state| state == "checked")
        );
        assert_eq!(
            node(&tree, "pref-font-lower").unwrap()["name"],
            "Decrease reading text size"
        );
        click(&mut runtime, "reading-level-technical");
        let tree = runtime.accessibility_json();
        assert!(
            node(&tree, "reading-level-technical").unwrap()["states"]
                .as_array()
                .unwrap()
                .iter()
                .any(|state| state == "checked")
        );
    }
}
