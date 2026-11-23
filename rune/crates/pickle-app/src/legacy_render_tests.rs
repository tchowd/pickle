//! The real app reader and pinned RuneKit renderer, not a Markdown AST oracle.
//! No services, native window, system clipboard, GPU or personal data is used.
use super::*;
use pickle_core::{ConversationTurn, QualityStatus};
use rune::{Env, Runtime};

const ORIGINAL: &str = "The design involves several key components:\n\n1.  **Model Inference**: The API will use a large language model.\n2.  **GPU Batching**: The model will be run on a pool of GPUs.\n10) Wide markers stay numbered";
const COPIED: &str = "The design involves several key components:\n\n1. Model Inference: The API will use a large language model.\n2. GPU Batching: The model will be run on a pool of GPUs.\n10. Wide markers stay numbered";

fn settle(runtime: &mut Runtime<AppState>) {
    for _ in 0..90 {
        runtime.frame(1.0 / 60.0);
    }
}

#[test]
fn original_list_markers_render_as_one_two_ten_and_real_copy_keeps_them() {
    for latest_turn in [false, true] {
        let mut state = AppState::default();
        state.sample_result();
        state.demo = false;
        if latest_turn {
            state.result = None;
            state.conversation.push(ConversationTurn {
                question: "Which components?".into(),
                answer: ORIGINAL.into(),
                quality: QualityStatus::NotChecked(QualityStatus::CHECKS_OFF.into()),
                label: None,
            });
        } else {
            state.result.as_mut().unwrap().text = ORIGINAL.into();
        }
        let source = state.snapshot.clone();
        let mut runtime = Runtime::new(state, view, (680.0, 1100.0), Env::new(true, 1.0));
        let clipboard = rune::clipboard::memory();
        runtime.use_clipboard(&clipboard);
        settle(&mut runtime);
        let elements = runtime.elements();
        let answer = elements
            .iter()
            .find(|element| element.key.as_deref() == Some("answer"))
            .expect("The production reader must mount RuneKit Markdown");
        let subtree = format!("{}/", answer.path);
        // RuneKit marks decorative numeric glyphs a11y_hidden. Inspect the
        // actual laid-out text elements that paint them, scoped to this answer.
        let markers: Vec<_> = elements
            .iter()
            .filter(|element| element.path.starts_with(&subtree))
            .filter(|element| {
                element.text.as_deref().is_some_and(|text| {
                    text.strip_suffix('.').is_some_and(|digits| {
                        !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
                    })
                })
            })
            .collect();
        assert_eq!(
            markers
                .iter()
                .map(|element| element.text.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["1.", "2.", "10."],
            "Pinned RuneKit must render each independent ordered-list start"
        );
        for marker in markers {
            assert!(marker.opacity > 0.99 && !marker.exiting);
            assert!(marker.rect[2] > 0.0 && marker.rect[3] > 0.0);
            assert!(marker.visible[2] > 0.0 && marker.visible[3] > 0.0);
            assert!(marker.rect[1] >= 48.0 && marker.rect[1] + marker.rect[3] <= 1100.0);
        }
        let (x, y) = runtime.find("copy-answer").expect("Actual Copy action");
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        settle(&mut runtime);
        assert_eq!(clipboard.text().unwrap().as_str(), COPIED);
        assert_eq!(runtime.state.answer(), ORIGINAL);
        assert_eq!(runtime.state.snapshot, source);
        assert!(runtime.state.services.is_none());
        assert!(!runtime.state.native_enabled);
    }
}
