use crate::controller::{Activity, AppState, Panel, Route};
use pickle_core::{ChartKind, ReadingAction, ValidatedChart};
use rune::{
    Color, Cx, El, ScrollSpec, TextEdit, Tx, col, el, row, spacer, text, text_input,
    tok::{TextStyle, type_},
};
use rune_kit::{Markdown, MarkdownStyle};

// Rune stores linear RGB. These decode the original Pickle sRGB palette.
const PAPER: Color = Color {
    r: 0.00350,
    g: 0.00657,
    b: 0.00542,
    a: 1.0,
};
const SURFACE: Color = Color {
    r: 0.00786,
    g: 0.01533,
    b: 0.01002,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.44799,
    g: 0.91141,
    b: 0.05088,
    a: 1.0,
};
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
const HAIR: Color = Color {
    r: 0.03968,
    g: 0.07829,
    b: 0.04667,
    a: 1.0,
};
const WARNING: Color = Color {
    r: 0.95510,
    g: 0.57048,
    b: 0.10654,
    a: 1.0,
};

pub fn button(
    s: &AppState,
    key: impl Into<String>,
    label: impl Into<String>,
    primary: bool,
    f: impl Fn(&mut AppState, &mut Tx) + 'static,
) -> El {
    let accent = accent(s);
    row()
        .key(key)
        .role("button")
        .label(label.into())
        .focusable(true)
        .h(34.0)
        .px(12.0)
        .center()
        .radius(8.0)
        .fill(if primary { accent } else { SURFACE })
        .border(1.0, if primary { accent } else { HAIR })
        .hover(if primary {
            accent.alpha(0.15)
        } else {
            accent.alpha(0.07)
        })
        .focus_ring(2.0, 2.0, accent)
        .on_press_tx(f)
}
pub(crate) fn action(
    s: &AppState,
    key: &str,
    label: &str,
    primary: bool,
    f: impl Fn(&mut AppState, &mut Tx) + 'static,
) -> El {
    button(s, key, label, primary, f).child(
        text(label)
            .font(TextStyle::new(13.0, 600.0))
            .color(if primary { PAPER } else { INK }),
    )
}
fn accent(s: &AppState) -> Color {
    GREEN.mix(INK, 1.0 - s.settings.theme_intensity.clamp(0.0, 1.0))
}
fn note(value: impl Into<String>) -> El {
    text(value)
        .wrap()
        .shrink()
        .font(TextStyle::new(12.0, 400.0).lh(18.0))
        .color(MUTED)
}
fn paragraph(value: impl Into<String>) -> El {
    text(value)
        .wrap()
        .shrink()
        .font(TextStyle::new(15.0, 400.0).lh(23.0))
        .color(INK)
}
fn section(title: &str) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(text(title).font(TextStyle::new(17.0, 600.0)).color(INK))
}
fn input(
    key: &str,
    label: &str,
    value: &TextEdit,
    edit: fn(&mut AppState) -> &mut TextEdit,
    secure: bool,
) -> El {
    input_with_submit(key, label, value, edit, secure, None)
}
fn input_with_submit(
    key: &str,
    label: &str,
    value: &TextEdit,
    edit: fn(&mut AppState) -> &mut TextEdit,
    secure: bool,
    submit: Option<fn(&mut AppState, &mut Tx)>,
) -> El {
    let field = text_input(value, TextStyle::new(14.0, 400.0).lh(22.0))
        .key(key)
        .label(label)
        .single_line()
        .w_full()
        .color(INK)
        .on_edit(edit);
    let field = if let Some(submit) = submit {
        field.on_submit(submit)
    } else {
        field
    };
    let field = if secure { field.secure() } else { field };
    col().w_full().gap(6.0).child(note(label)).child(
        el().w_full()
            .h(38.0)
            .px(10.0)
            .center_y()
            .radius(7.0)
            .fill(PAPER)
            .border(1.0, HAIR)
            .child(field),
    )
}
fn submit_follow_up(s: &mut AppState, tx: &mut Tx) {
    if s.reading_busy() {
        return;
    }
    s.follow_up(s.question.text.clone(), None, tx);
}
fn submit_term(s: &mut AppState, tx: &mut Tx) {
    s.explain_term(s.term.text.clone(), tx);
}
fn toggle(
    s: &AppState,
    key: &str,
    title: &str,
    on: bool,
    f: impl Fn(&mut AppState, &mut Tx) + 'static,
) -> El {
    row()
        .w_full()
        .gap(10.0)
        .center_y()
        .child(paragraph(title).grow(1.0))
        .child(
            action(s, key, if on { "On" } else { "Off" }, on, f)
                .role("switch")
                .label(title)
                .checked(on),
        )
}
pub fn view(s: &AppState, _: &mut Cx<AppState>) -> El {
    let mut root = col()
        .key("pickle-window")
        .role("window")
        .label("Pickle reading assistant")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(PAPER.alpha(s.settings.background_opacity))
        .radius(16.0)
        .border(1.0, HAIR);
    let titlebar = row()
        .key("drag-region")
        .h(48.0)
        .px(18.0)
        .gap(9.0)
        .center_y()
        .window_drag()
        .child(el().size(9.0, 20.0).radius(5.0).fill(GREEN))
        .child(text("Pickle").font(TextStyle::new(18.0, 650.0)).color(INK))
        .child(spacer())
        .child(
            action(
                s,
                "pin-reader",
                if s.pinned { "Unpin" } else { "Pin" },
                s.pinned,
                |s, _| s.toggle_pin(),
            )
            .label("Keep the reader on top and preserve it during automatic selection capture")
            .checked(s.pinned),
        )
        .child(action(s, "open-saved", "Saved", false, |s, tx| {
            s.dismiss_auto_preview();
            s.route = Route::Library(true);
            s.library_selected = None;
            s.library_page = 0;
            s.expand(tx, 680.0);
        }))
        .child(action(s, "open-settings", "Settings", false, |s, tx| {
            s.dismiss_auto_preview();
            s.route = Route::Settings;
            s.expand(tx, 720.0);
        }))
        .child(
            action(s, "close", "×", false, |s, tx| {
                s.prepare_window_close();
                s.cancel();
                tx.close_window();
            })
            .label("Close reader"),
        );
    root = root.child(titlebar).child(el().h(1.0).w_full().fill(HAIR));
    root = root.child(match s.route {
        Route::Reader => reader(s),
        Route::Settings => settings(s),
        Route::Library(bookmark) => library(s, bookmark),
    });
    // A native system material replaces its own fill. Paint the adjustable tint
    // on a separate content child so it remains present above the native view.
    col()
        .key("pickle-material")
        .w_full()
        .h_full()
        .min_h(0.0)
        .radius(16.0)
        .system_material(rune::SystemMaterial::Popover)
        .child(root)
}
fn reader(s: &AppState) -> El {
    if s.panel == Panel::Compose {
        return compose(s);
    }
    let app = s
        .display_snapshot()
        .map(|snapshot| snapshot.app_name.as_str())
        .unwrap_or("Your passage");
    let source = s
        .display_snapshot()
        .map(|snapshot| snapshot.text.as_str())
        .unwrap_or("");
    let mut source_text = text(if s.source_expanded {
        source.to_owned()
    } else {
        source
            .chars()
            .take(if s.panel == Panel::Choosing { 210 } else { 450 })
            .collect()
    })
    .font(TextStyle::new(14.0, 400.0).lh(21.0))
    .wrap_anywhere()
    .w_full()
    .shrink()
    .color(INK)
    .selectable(true);
    if !s.source_expanded {
        source_text = source_text.max_lines(if s.panel == Panel::Choosing { 2 } else { 8 });
    }
    let mut source_panel = col()
        .key("source-preview")
        .w_full()
        .gap(6.0)
        .padding(14.0)
        .radius(10.0)
        .fill(SURFACE)
        .child(source_text);
    if !source.is_empty() {
        source_panel = source_panel.child(action(
            s,
            "toggle-full-source",
            if s.source_expanded {
                "Hide full passage"
            } else {
                "Show full passage"
            },
            false,
            |s, tx| {
                s.source_expanded = !s.source_expanded;
                if s.source_expanded {
                    s.expand(tx, 680.0);
                }
            },
        ));
    }
    let mut content = col()
        .w_full()
        .gap(16.0)
        .padding(22.0)
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(note(app))
                .child(spacer())
                .child(note(if s.demo && s.auto_preview.is_none() {
                    "Fixed demonstration · no network"
                } else if s.settings.mode == "Online" {
                    "Online · Cloudflare"
                } else if s.settings.same_device {
                    "Local · this device"
                } else {
                    "Local · configured server"
                })),
        )
        .child(source_panel);
    let busy = s.reading_busy();
    content = content.child(
        row()
            .w_full()
            .gap(8.0)
            .child(
                action(s, "simplify", "Simplify", true, |s, tx| {
                    s.run_action(ReadingAction::Simplify, tx)
                })
                .disabled(busy),
            )
            .child(
                action(s, "expand", "Explain more", false, |s, tx| {
                    s.run_action(ReadingAction::Expand, tx)
                })
                .disabled(busy),
            )
            .child(
                action(s, "chart", "Visualize", false, |s, tx| {
                    s.run_action(ReadingAction::Chart, tx)
                })
                .disabled(busy),
            )
            .child(spacer())
            .child(
                action(s, "new-passage", "New", false, |s, tx| {
                    s.reset();
                    s.expand(tx, 520.0);
                })
                .label("Start a new passage"),
            ),
    );
    content = content
        .child(input_with_submit("explain-term", "Word or phrase from this passage", &s.term, |s| &mut s.term, false, Some(submit_term)))
        .child(row().gap(8.0)
            .child(action(s, "explain-term-submit", "Explain word or phrase", false, submit_term).disabled(busy || s.settings_pending))
            .child(action(s, "explain-selection", "Explain selected words", false, |s, tx| {
                if let Some(selection) = tx.text_selection() { s.explain_term(selection.text().to_owned(), tx); }
                else { s.notice = Some("Select a word in the passage or answer, then choose Explain selected words.".into()); }
            }).disabled(busy || s.settings_pending).keeps_text_selection()));
    if s.panel == Panel::Expanded {
        content = content.child(activity(s));
        if let Some(result) = &s.result {
            content = content.child(
                text(match result.action {
                    ReadingAction::Simplify => "A little clearer.",
                    ReadingAction::Expand => "The bigger picture.",
                    ReadingAction::Chart => "See the connection.",
                    ReadingAction::FollowUp => "Let’s go deeper.",
                })
                .font(TextStyle::new(24.0, 600.0).lh(31.0))
                .color(INK),
            );
            if let Some(chart) = &result.chart {
                content = content.child(chart_view(chart));
            }
            if result.chart.is_none() {
                content = content.child(answer(&result.text, s.settings.text_size, false));
            }
            if let Some(label) = result.quality.visible_label() {
                content = content.child(note(label));
            }
            if !s.demo {
                let model = pickle_core::bounded_utf8(&result.model, 160);
                let model = if model.is_empty() {
                    "Unreported"
                } else {
                    model
                };
                let timing = if result.elapsed.is_finite() && result.elapsed >= 0.0 {
                    format!(" · {:.1}s total", result.elapsed)
                } else {
                    String::new()
                };
                content = content
                    .child(note(format!("Answer model: {model}{timing}")).key("answer-provenance"));
            }
        }
        for (i, turn) in s.conversation.iter().enumerate() {
            content = content.child(
                col()
                    .key(format!("turn-{i}"))
                    .w_full()
                    .gap(12.0)
                    .child(el().h(1.0).w_full().fill(HAIR))
                    .child(
                        text(turn.label.as_ref().unwrap_or(&turn.question))
                            .wrap_anywhere()
                            .shrink()
                            .font(TextStyle::new(15.0, 600.0))
                            .color(GREEN),
                    )
                    .child(answer(&turn.answer, s.settings.text_size, false)),
            );
            if let Some(label) = turn.quality.visible_label() {
                content = content.child(note(label));
            }
        }
        if !s.draft.is_empty() {
            content = content.child(answer(&s.draft, s.settings.text_size, true));
        }
        if (s.result.is_some() || !s.conversation.is_empty()) && !busy {
            content = content.child(
                row()
                    .gap(8.0)
                    .child(action(s, "copy-answer", "Copy", false, |s, tx| {
                        tx.copy(pickle_core::answer_plain_text(s.answer()))
                    }))
                    .child(action(s, "save-answer", "Save", false, |s, _| {
                        s.save_answer()
                    }))
                    .child(action(s, "retry", "Retry", false, |s, tx| {
                        if let Some(pending) = s.last.clone() {
                            s.start(pending, tx);
                        }
                    })),
            );
            content = content.child(row().gap(8.0)
                .child(action(s, "shorter", "Shorter", false, |s, tx| s.follow_up("Make the explanation shorter without losing qualifications, uncertainty, or important details.".into(), Some("Shorter".into()), tx)))
                .child(action(s, "more-detail", "More detail", false, |s, tx| s.follow_up("Explain this in more detail, distinguishing source facts from general background.".into(), Some("More detail".into()), tx)))
                .child(action(s, "example", "Give an example", false, |s, tx| s.follow_up("Give a clearly labeled hypothetical example that illustrates this passage without inventing source-specific facts.".into(), Some("Give an example".into()), tx))));
            content = content
                .child(input_with_submit(
                    "follow-up",
                    "Ask about this passage",
                    &s.question,
                    |s| &mut s.question,
                    false,
                    Some(submit_follow_up),
                ))
                .child(row().gap(8.0).child(action(
                    s,
                    "send-follow-up",
                    "Ask",
                    true,
                    submit_follow_up,
                )));
            if !s.conversation.is_empty() {
                content = content.child(
                    action(
                        s,
                        "clear-follow-ups",
                        "Clear follow-ups (keep passage)",
                        false,
                        |s, _| s.clear_follow_ups(),
                    )
                    .disabled(s.settings_pending),
                );
            }
        }
    }
    if s.auto_preview.is_none()
        && (s.context_state.has_content()
            || s.context_state.pending > 0
            || s.capture_target.is_some())
    {
        content = content.child(context_tools(s));
    }
    if let Some(notice) = &s.notice {
        content = content.child(note(notice));
    }
    content = content.child(note(if s.settings.history_enabled {
        "Knowledge base on · completed answers are saved on this device"
    } else {
        "History is off · answers are saved only when you choose Save"
    }));
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}
fn compose(s: &AppState) -> El {
    let field = text_input(&s.source, TextStyle::new(16.0, 400.0).lh(25.0))
        .key("passage-input")
        .label("Passage to read")
        .w_full()
        .color(INK)
        .placeholder("Paste a passage here…", MUTED)
        .on_edit(|s: &mut AppState| &mut s.source);
    let mut content = col().w_full().gap(18.0).padding(24.0)
        .child(text("Make room for understanding.").font(TextStyle::new(27.0, 600.0).lh(35.0)).color(INK))
        .child(paragraph("Highlight a passage in another app and invoke Pickle, or paste something below."))
        .child(el().w_full().min_h(150.0).padding(14.0).radius(10.0).fill(SURFACE).border(1.0, HAIR).child(field))
        .child(row().gap(10.0)
            .child(action(s, "use-passage", "Read this passage", true, |s, tx| { s.commit_source(); if s.snapshot.is_some() { s.expand(tx, 300.0); } }))
            .child(action(s, "sample", "Try a sample", false, |s, tx| { s.sample(); s.expand(tx, 300.0); })))
        .child(note(if cfg!(target_os = "macos") && s.settings.modifier_gesture {
            "Shortcut: press and release Control + Option. Selection capture needs Accessibility access.".to_string()
        } else { format!("Shortcut: {}. Some applications do not expose selected text; paste remains available.", s.settings.shortcut) }));
    if let Activity::Failed(error) = &s.activity {
        content = content.child(text(error).font(type_::BODY).color(WARNING));
    }
    if let Some(notice) = &s.notice {
        content = content.child(note(notice));
    }
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}
fn answer(value: &str, size: f32, streaming: bool) -> El {
    Markdown::new(pickle_core::safe_answer_markup(value))
        .autolink(false)
        .selectable(true)
        .streaming(streaming)
        .render(
            &MarkdownStyle::neutral(true)
                .text(INK)
                .muted(MUTED)
                .body_font(TextStyle::new(size, 400.0).lh(size * 1.55))
                .selection(GREEN.alpha(0.22)),
        )
        .key(if streaming { "answer-draft" } else { "answer" })
        .w_full()
}
fn activity(s: &AppState) -> El {
    match &s.activity {
        Activity::Idle => el(),
        Activity::WaitingContext => row().gap(12.0).child(note("Reading available context…")).child(action(s, "cancel-context-wait", "Cancel", false, |s, _| s.cancel())),
        Activity::Running { progress, .. } => row().gap(12.0).center_y().child(text(progress).font(type_::BODY).color(GREEN)).child(spacer()).child(action(s, "cancel-request", "Cancel", false, |s, _| s.cancel())),
        Activity::Failed(error) => col().w_full().gap(12.0).padding(14.0).radius(10.0).fill(SURFACE)
            .child(text(error).font(TextStyle::new(14.0, 400.0).lh(21.0)).color(WARNING))
            .child(row().gap(8.0).child(action(s, "retry-error", "Retry", true, |s, tx| if let Some(p) = s.last.clone() { s.start(p, tx); }))
                .child(action(s, "error-settings", "Connection settings", false, |s, _| s.route = Route::Settings))),
        Activity::Consent(_) => {
            let destination = if s.settings.mode == "Online" { if s.settings.jev_enabled { "Cloudflare for writing, and TypeSafe Jev through Cloudflare for answer checks".to_owned() } else { "Cloudflare for writing".into() } }
                else { format!("your configured Ollama server at {}{}", s.settings.local_address, if s.settings.same_device { " on this device" } else { " (which may run on another computer)" }) };
            section("Before sharing this passage").padding(14.0).radius(10.0).fill(SURFACE)
                .child(paragraph(format!("Your selected passage, supplied context, question, and relevant conversation will be sent to {destination}. Attached OCR, page references, transcripts, and visual summaries are included when present; screenshots are sent only by the explicit visual-analysis action. Requests may incur provider charges. Saved history is never included.")))
                .child(row().gap(8.0).child(action(s, "consent-continue", "Continue", true, |s, tx| s.consent(tx)))
                    .child(action(s, "consent-cancel", "Cancel", false, |s, _| s.cancel())))
        }
        Activity::ChooseChart(_) => section("Choose a source-backed visual")
            .child(paragraph("Bar charts need explicit quantities with matching units. Flow diagrams need relationships stated in the passage."))
            .child(row().gap(8.0)
                .child(action(s, "choose-bar", "Bar chart", true, |s, tx| if let Activity::ChooseChart(p) = &s.activity { let mut p = p.clone(); p.chart = Some(ChartKind::Bar); s.start(p, tx); }))
                .child(action(s, "choose-flow", "Flow diagram", false, |s, tx| if let Activity::ChooseChart(p) = &s.activity { let mut p = p.clone(); p.chart = Some(ChartKind::Flow); s.start(p, tx); }))),
        Activity::NeedsContext(_, flags) => section("A little more context may help")
            .child(paragraph(flags.join(", ").replace('_', " ")))
            .child(input("additional-context", "Add surrounding text", &s.context, |s| &mut s.context, false))
            .child(row().gap(8.0).child(action(s, "retry-context", "Use this context", true, |s, tx| if let Activity::NeedsContext(p, _) = &s.activity { let p = p.clone(); s.start(p, tx); }))
                .child(action(s, "limited-context", "Explain with limits", false, |s, tx| if let Activity::NeedsContext(p, _) = &s.activity { let mut p = p.clone(); p.limited = true; s.start(p, tx); }))),
    }
}
fn chart_view(chart: &ValidatedChart) -> El {
    match chart {
        ValidatedChart::Bar(values) => {
            let max = values
                .iter()
                .map(|v| v.value.abs())
                .fold(0.0_f64, f64::max)
                .max(1.0);
            let mut bars = col()
                .key("bar-chart")
                .role("group")
                .label("Source-backed bar chart")
                .w_full()
                .gap(14.0);
            for (i, value) in values.iter().enumerate() {
                let width = (value.value.abs() / max * 170.0) as f32;
                bars = bars.child(
                    col()
                        .key(format!("bar-{i}"))
                        .gap(5.0)
                        .child(note(value.excerpt.clone()))
                        .child(
                            row()
                                .gap(0.0)
                                .center_y()
                                .child(
                                    row().w(172.0).h(24.0).child(spacer()).child(
                                        el().w(if value.value < 0.0 { width } else { 0.0 })
                                            .h(18.0)
                                            .radius(3.0)
                                            .fill(WARNING),
                                    ),
                                )
                                .child(el().w(1.0).h(25.0).fill(MUTED))
                                .child(
                                    row().w(172.0).h(24.0).center_y().child(
                                        el().w(if value.value >= 0.0 { width } else { 0.0 })
                                            .h(18.0)
                                            .radius(3.0)
                                            .fill(GREEN),
                                    ),
                                ),
                        )
                        .child(paragraph(format!("{} {}", value.value, value.unit))),
                );
            }
            bars.child(note(
                "Zero baseline · each quantity comes from the selected passage",
            ))
        }
        ValidatedChart::Flow { nodes, edges } => {
            let mut flow = col()
                .key("flow-chart")
                .role("group")
                .label("Source-backed relationships")
                .w_full()
                .gap(14.0);
            for (i, edge) in edges.iter().enumerate() {
                let from = nodes
                    .iter()
                    .find(|n| n.id == edge.from)
                    .map(|n| n.label.as_str())
                    .unwrap_or("");
                let to = nodes
                    .iter()
                    .find(|n| n.id == edge.to)
                    .map(|n| n.label.as_str())
                    .unwrap_or("");
                flow = flow.child(
                    col()
                        .key(format!("edge-{i}"))
                        .gap(8.0)
                        .padding(14.0)
                        .radius(8.0)
                        .fill(SURFACE)
                        .child(paragraph(format!("{from}  →  {to}")))
                        .child(note(if edge.condition.is_empty() {
                            edge.evidence.clone()
                        } else {
                            format!("When: {}\nSource: {}", edge.condition, edge.evidence)
                        })),
                );
            }
            flow
        }
    }
}
fn invocation_recovery(s: &AppState) -> El {
    if !s.invocation_retry_visible() {
        return el();
    }
    col()
        .w_full()
        .gap(8.0)
        .child(note("Shortcut registration did not complete. After resolving the system error, retry the saved shortcut. Pasting remains available."))
        .child(
            action(s, "retry-invocation", "Retry shortcut", false, |s, _| s.retry_invocation())
                .disabled(s.paused || s.settings_pending),
        )
}

fn settings(s: &AppState) -> El {
    if s.settings_pending {
        return col()
            .w_full()
            .grow(1.0)
            .padding(24.0)
            .gap(12.0)
            .child(paragraph("Saving settings…"))
            .child(note(
                "Your active connection changes only after the saved configuration is accepted.",
            ));
    }
    let mut content = col()
        .w_full()
        .gap(24.0)
        .padding(24.0)
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(
                    text("Settings")
                        .font(TextStyle::new(25.0, 600.0))
                        .color(INK),
                )
                .child(spacer())
                .child(action(s, "back-reader", "Back", false, |s, _| {
                    s.route = Route::Reader
                })),
        )
        .child(
            section("Where answers are created").child(
                row()
                    .gap(8.0)
                    .child(action(
                        s,
                        "mode-local",
                        "Local",
                        s.settings.mode == "Local",
                        |s, _| s.change_mode("Local"),
                    ))
                    .child(action(
                        s,
                        "mode-online",
                        "Online",
                        s.settings.mode == "Online",
                        |s, _| s.change_mode("Online"),
                    )),
            ),
        );
    content = if s.settings.mode == "Local" {
        content.child(section("Local connection")
            .child(input("local-address", "Ollama server address", &s.local_address, |s| &mut s.local_address, false))
            .child(input("local-model", "Downloaded model", &s.local_model, |s| &mut s.local_model, false))
            .child(crate::local_connection::controls(s))
            .child(note("Review factual claims against the passage. Local prose does not receive the optional answer checks."))
            .child(input("local-vision-model", "Downloaded vision model (optional)", &s.local_vision_model, |s| &mut s.local_vision_model, false))
            .child(toggle(s, "same-device", "This server runs on this device", s.settings.same_device, |s, _| { s.cancel(); s.settings.same_device = !s.settings.same_device; s.settings.local_consent.clear(); }))
            .child(note("A loopback SSH tunnel can lead to another computer. Enable this only for inference actually running on this device.")))
    } else {
        content.child(section("Cloudflare connection")
            .child(input("cloud-account", "Account ID", &s.account_id, |s| &mut s.account_id, false))
            .child(input("cloud-model", "Writing model", &s.cloud_model, |s| &mut s.cloud_model, false))
            .child(toggle(s, "jev-enabled", "Check answers with Jev", s.settings.jev_enabled, |s, _| { s.cancel(); s.settings.jev_enabled = !s.settings.jev_enabled; }))
            .child(note("Jev sends the source and candidate answer to TypeSafe through Cloudflare. Checks are model judgments, not fact verification.")))
    };
    content = content.child(section("Credentials")
        .child(input("credential", if s.settings.mode == "Online" { "Cloudflare API token" } else { "Local server access key (if required)" }, &s.credential, |s| s.edit_credential(), true))
        .child(row().gap(8.0).child(action(s, "save-credential", "Save credential", false, |s, _| s.save_credential(false)).disabled(s.credential_busy()))
            .child(action(s, "remove-credential", "Remove saved credential", false, |s, _| s.save_credential(true)).disabled(s.credential_busy())))
        .child(action(s, "authorize-credential", "Authorize saved credential…", false, |s, _| s.authorize_credential()).disabled(s.credential_busy()))
        .child(note(if s.credential_busy() { "Waiting for the system credential operation to finish…" } else { "Keys belong to this saved endpoint or Cloudflare account. Older unscoped cloud keys require re-entry; they are never reused or removed automatically." })))
        .child(section("Privacy and reading")
            .child(toggle(s, "strict-offline", "Strict offline mode", s.settings.strict_offline, |s, _| { s.cancel(); s.cancel_update(); s.settings.strict_offline = !s.settings.strict_offline; }))
            .child(note("Blocks cloud AI and inference on another computer, including tunneled servers."))
            .child(toggle(s, "history-enabled", "Keep a knowledge base on this device", s.settings.history_enabled, |s, _| s.settings.history_enabled = !s.settings.history_enabled))
            .child(note("Records completed answers and their selected passages. Turning this off keeps existing entries. History is never sent to the AI."))
            .child(toggle(s, "streaming-enabled", "Show answers as they arrive", s.settings.streaming, |s, _| { s.cancel(); s.settings.streaming = !s.settings.streaming; }))
            .child(toggle(s, "page-context-enabled", "Capture source-window text with local OCR", s.settings.screen_context_enabled, |s, _| { s.clear_context(); s.settings.screen_context_enabled = !s.settings.screen_context_enabled; s.settings.context_consent_destination.clear(); }))
            .child(toggle(s, "web-context-enabled", "Include webpage and browser references", s.settings.web_context_enabled, |s, _| { s.clear_context(); s.settings.web_context_enabled = !s.settings.web_context_enabled; s.settings.context_consent_destination.clear(); }))
            )
        .child(crate::preferences_view::reading_and_appearance(s))
        .child(section("Invocation")
            .child(input("invocation-shortcut", "Global shortcut (for example ctrl+alt+p)", &s.shortcut, |s| &mut s.shortcut, false))
            .child(note("Save settings to register a new shortcut. On macOS, turn off the modifier gesture to use it. A conflicting shortcut leaves the previous registration active."))
            .child(invocation_recovery(s))
            .child(toggle(s, "modifier-gesture", "Control + Option press and release on macOS", s.settings.modifier_gesture, |s, _| s.set_modifier_gesture(!s.settings.modifier_gesture)).disabled(!cfg!(target_os = "macos")))
            .child(toggle(s, "automatic-menu", "Show the chooser after selecting text", s.settings.automatic_menu, |s, _| s.set_automatic_menu(!s.settings.automatic_menu)))
            .child(note("Automatic previews do not replace your answer until you choose an action. Pin the reader to suppress them.")))
        .child(section("Selection capture")
            .child(note("Pickle reads selections exposed by the source application. Protected fields and excluded applications are blocked."))
            .child(action(s, "accessibility-permission", "Request selection access", false, |s, _| { if s.native_enabled { pickle_platform::request_accessibility(); s.notice = Some("Review Pickle's selection permission in system settings, then try the shortcut again.".into()); } }).disabled(!s.native_enabled))
            .child(action(s, "screen-permission", "Allow screen capture", false, |s, _| { if s.native_enabled { pickle_platform::request_screen_capture(); s.notice = Some("Review the system's screen-capture permission, then invoke Pickle again.".into()); } }).disabled(!s.native_enabled))
            .child(note("Excluded application IDs or Windows executable names, one per line"))
            .child(el().w_full().h(80.0).padding(10.0).radius(7.0).fill(PAPER).border(1.0, HAIR).child(text_input(&s.exclusions, type_::BODY).key("excluded-apps").label("Excluded application IDs or Windows executable names").color(INK).on_edit(|s: &mut AppState| &mut s.exclusions))))
        .child(section("Local audio transcription on Windows")
            .child(note("Choose a trusted whisper.cpp executable and an already downloaded model. Audio is passed in memory; no microphone, cloud service, or automatic model download is used. macOS uses its available on-device speech recognizer."))
            .child(input("transcriber-executable", "Absolute path to whisper-cli.exe", &s.transcriber_executable, |s| &mut s.transcriber_executable, false))
            .child(input("transcriber-model", "Absolute path to the downloaded model", &s.transcriber_model, |s| &mut s.transcriber_model, false))
            .child(input("transcriber-language", "Language code (auto or en, for example)", &s.transcriber_language, |s| &mut s.transcriber_language, false)))
        .child(section("Browser extension")
            .child(note("Open the bundled extension folder. In Chrome or Edge, load it from the extensions page using Developer mode, then paste its extension ID below. Browser references must be enabled separately."))
            .child(action(s, "show-extension", "Show extension folder", false, |s, _| s.reveal_extension()))
            .child(input("extension-id", "Extension ID", &s.extension_id, |s| &mut s.extension_id, false))
            .child(row().gap(8.0).child(action(s, "connect-chrome", "Connect Chrome", false, |s, _| s.connect_browser(false)))
                .child(action(s, "connect-edge", "Connect Edge", false, |s, _| s.connect_browser(true)))))
        .child(row().gap(8.0)
            .child(action(s, "disconnect-chrome", "Disconnect Chrome", false, |s, _| s.disconnect_browser(false)))
            .child(action(s, "disconnect-edge", "Disconnect Edge", false, |s, _| s.disconnect_browser(true))))
        .child(update_tools(s))
        .child(diagnostics_tools(s))
        .child(section("Existing Pickle data")
            .child(note("Import saved answers and knowledge-base history from the original Mac app. Original files, credentials, and privacy consents are preserved."))
            .child(action(s, "import-legacy", "Import original Pickle history", false, |s, _| s.import_legacy()).disabled(!cfg!(target_os = "macos"))))
        .child(row().gap(8.0).child(action(s, "save-settings", "Save settings", true, |s, _| s.save_settings()))
            .child(action(s, "open-history", "Knowledge base", false, |s, _| s.route = Route::Library(false)))
            .child(action(s, "pause", if s.paused { "Resume" } else { "Pause" }, false, |s, _| s.toggle_pause())));
    if let Some(notice) = &s.notice {
        content = content.child(note(notice));
    }
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}
pub(crate) fn diagnostics_tools(s: &AppState) -> El {
    use crate::diagnostics::Status;
    let busy = s.diagnostics.status.busy();
    let mut panel = section("Support diagnostics")
        .child(note("Review a report containing only build details, platform capability and permission flags, and reading-policy switches. No passage, answers, context, history, connection addresses, model names, credentials or application exclusions are included. Nothing is sent automatically."))
        .child(action(s, "review-diagnostics", if s.diagnostics.report.is_some() { "Refresh report preview" } else { "Review diagnostics" }, false, |s, _| s.review_diagnostics()).disabled(busy));
    if let Some(report) = &s.diagnostics.report {
        panel = panel
            .child(
                col()
                    .key("diagnostics-preview")
                    .role("group")
                    .label("Complete diagnostics report preview")
                    .w_full()
                    .h(260.0)
                    .padding(12.0)
                    .radius(8.0)
                    .fill(PAPER)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        text(report.json())
                            .font(TextStyle::new(12.0, 400.0).lh(18.0))
                            .color(INK)
                            .selectable(true),
                    ),
            )
            .child(
                action(
                    s,
                    "export-diagnostics",
                    "Export this report…",
                    false,
                    |s, tx| s.export_diagnostics(tx),
                )
                .disabled(busy),
            );
    }
    match &s.diagnostics.status {
        Status::Choosing => panel.child(note(
            "Choose a destination in the save dialog. Canceling writes no file.",
        )),
        Status::Writing => panel.child(note("Saving the reviewed report…")),
        Status::Exported => panel.child(note(
            "Report exported. You can choose whether to share the file with support.",
        )),
        Status::Failed(message) => panel.child(note(*message)),
        _ => panel,
    }
}

fn update_tools(s: &AppState) -> El {
    use crate::update_ui::UpdateState;
    let panel = section(&format!("Pickle Rune {}", env!("CARGO_PKG_VERSION")));
    match &s.update_state {
        UpdateState::Idle => panel.child(note("Updates are checked only when you choose. Downloads must match the publisher's signature and the signed artifact checksum."))
            .child(action(s, "check-updates", "Check for updates", false, |s, _| s.check_updates())),
        UpdateState::Checking(_) | UpdateState::Downloading(_) => panel.child(note(if matches!(s.update_state, UpdateState::Checking(_)) { "Checking the signed release feed…" } else { "Downloading and verifying the update…" }))
            .child(action(s, "cancel-update", "Cancel update request", false, |s, _| s.cancel_update())),
        UpdateState::Available(release) => panel.child(paragraph(format!("Version {} is available.", release.version)))
            .child(note(format!("{} channel · published {} · {:.1} MB", release.channel, release.published_at, release.artifact().size as f64 / 1_048_576.0))).child(note(release.notes.clone()))
            .child(action(s, "download-update", "Download signed update", true, |s, _| s.download_update())),
        UpdateState::Ready(path) => panel.child(note(format!("Verified download: {}", path.display())))
            .child(note("Quit Pickle before replacing it. Open the verified installer/archive and follow the platform's installation steps. Your saved answers are kept."))
            .child(action(s, "show-update", "Show verified download", false, |s, _| s.reveal_update())),
        UpdateState::Current => panel.child(note("This build is up to date.")).child(action(s, "recheck-updates", "Check again", false, |s, _| s.check_updates())),
        UpdateState::Failed(error) => panel.child(note(error)).child(action(s, "retry-update", "Check again", false, |s, _| s.check_updates())),
    }
}
fn context_tools(s: &AppState) -> El {
    let mut context = section("Session context")
        .padding(14.0)
        .radius(10.0)
        .fill(SURFACE);
    if s.context_state.pending > 0 {
        context = context.child(note("Reading or recording context…"));
    }
    if s.context_state.has_content() {
        context = context.child(action(
            s,
            "preview-full-context",
            if s.context_state.preview_full {
                "Collapse context preview"
            } else {
                "Preview full context"
            },
            false,
            |s, _| s.context_state.preview_full = !s.context_state.preview_full,
        ));
    }
    if let Some(page) = &s.context_state.page {
        if s.context_state.preview_full
            && let Some(image) = &s.context_state.page_image
        {
            context = context.child(
                el().key("source-screenshot")
                    .role("img")
                    .label("Session-only screenshot of the source window")
                    .w_full()
                    .h(220.0)
                    .pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
            );
        }
        context = context.child(note(if page.truncated { "Source-window OCR excerpt (truncated)" } else { "Source-window OCR (may contain errors)" }))
            .child(text(if s.context_state.preview_full { page.text.clone() } else { page.text.chars().take(500).collect::<String>() }).wrap().shrink().font(type_::BODY).color(MUTED).selectable(true))
            .child(note(if s.settings.mode == "Online" { "Analyze visuals explicitly sends this session's screenshot and selected text to Cloudflare. Charges may apply.".to_owned() } else { format!("Analyze visuals explicitly sends this session's screenshot and selected text to your Local vision model at {}.", s.settings.local_address) }))
            .child(action(s, "analyze-visuals", if s.settings.mode == "Online" { "Analyze visuals with Cloudflare" } else { "Analyze visuals locally" }, false, |s, _| s.begin_visual_analysis()).disabled(s.context_state.pending > 0));
    }
    if let Some(reference) = &s.context_state.reference {
        context = context
            .child(note(format!("{} · {}", reference.title, reference.kind)))
            .child(
                text(if s.context_state.preview_full {
                    reference.text.clone()
                } else {
                    reference.text.chars().take(500).collect::<String>()
                })
                .wrap()
                .shrink()
                .font(type_::BODY)
                .color(MUTED)
                .selectable(true),
            );
    }
    if !s.context_state.visual_summary.is_empty() {
        context = context.child(note(format!(
            "Unverified visual interpretation: {}",
            s.context_state.visual_summary
        )));
    }
    if !s.context_state.audio_text.is_empty() {
        context = context.child(note(format!(
            "Local audio transcript: {}",
            s.context_state.audio_text
        )));
    }
    if let Some(notice) = &s.context_state.notice {
        context = context.child(note(notice));
    }
    context.child(
        row()
            .gap(8.0)
            .child(action(
                s,
                "remove-context",
                "Remove context",
                false,
                |s, _| s.clear_context(),
            ))
            .child(
                action(s, "listen", "Listen for 30 seconds", false, |s, _| {
                    s.begin_audio()
                })
                .disabled(
                    s.context_state.pending > 0 || !pickle_platform::capabilities().source_audio,
                ),
            ),
    )
}
fn library(s: &AppState, bookmark: bool) -> El {
    let entries = if bookmark { &s.bookmarks } else { &s.entries };
    let mut content = col()
        .w_full()
        .gap(18.0)
        .padding(24.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    text(if bookmark {
                        "Saved answers"
                    } else {
                        "Knowledge base"
                    })
                    .font(TextStyle::new(24.0, 600.0))
                    .color(INK),
                )
                .child(spacer())
                .child(action(s, "library-back", "Back", false, |s, _| {
                    s.route = Route::Reader
                })),
        )
        .child(
            row()
                .gap(8.0)
                .child(action(s, "library-bookmarks", "Saved", bookmark, |s, _| {
                    s.route = Route::Library(true);
                    s.library_selected = None;
                    s.library_page = 0;
                }))
                .child(action(
                    s,
                    "library-history",
                    "Knowledge base",
                    !bookmark,
                    |s, _| {
                        s.route = Route::Library(false);
                        s.library_selected = None;
                        s.library_page = 0;
                    },
                )),
        );
    if let Some(entry) = s
        .library_selected
        .as_ref()
        .and_then(|id| entries.iter().find(|entry| &entry.id == id))
    {
        let copied = pickle_core::answer_plain_text(&entry.answer);
        content = content
            .child(action(s, "entry-back", "Back to results", false, |s, _| {
                s.library_selected = None
            }))
            .child(note(format!("{} · {}", entry.app_name, entry.recorded_at)))
            .child(answer(&entry.answer, s.settings.text_size, false))
            .child(
                section("Original passage")
                    .child(paragraph(entry.passage.clone()).selectable(true)),
            )
            .child(action(
                s,
                "copy-saved-detail",
                "Copy answer",
                false,
                move |_, tx| tx.copy(copied.clone()),
            ));
        return col()
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(content);
    }
    let search = text_input(&s.search, TextStyle::new(14.0, 400.0).lh(22.0))
        .key("search-library")
        .label("Search passages and answers")
        .single_line()
        .w_full()
        .color(INK)
        .on_edit(|s: &mut AppState| &mut s.search)
        .on_input_change(|s: &mut AppState, _| s.library_page = 0);
    content = content.child(
        col()
            .w_full()
            .gap(6.0)
            .child(note("Search passages and answers"))
            .child(
                el().w_full()
                    .h(38.0)
                    .px(10.0)
                    .center_y()
                    .radius(7.0)
                    .fill(PAPER)
                    .border(1.0, HAIR)
                    .child(search),
            ),
    );
    if entries.is_empty() {
        content = content.child(paragraph(if bookmark {
            "Choose Save beside an answer to keep it here."
        } else {
            "Enable the knowledge base in Settings to record completed answers on this device."
        }));
    }
    let matches: Vec<_> = entries
        .iter()
        .filter(|entry| entry.matches(&s.search.text))
        .collect();
    let pages = matches.len().div_ceil(20).max(1);
    let page = s.library_page.min(pages - 1);
    if !entries.is_empty() && matches.is_empty() {
        content = content.child(paragraph("No saved answers match this search."));
    }
    for entry in matches.iter().skip(page * 20).take(20) {
        let id = entry.id.clone();
        let selected = entry.id.clone();
        let answer_text = pickle_core::answer_plain_text(&entry.answer);
        content = content.child(
            col()
                .key(format!("entry-{}", entry.id))
                .w_full()
                .gap(10.0)
                .padding(16.0)
                .radius(10.0)
                .fill(SURFACE)
                .child(note(format!("{} · {}", entry.app_name, entry.action)))
                .child(answer(
                    &entry.answer.chars().take(500).collect::<String>(),
                    s.settings.text_size,
                    false,
                ))
                .child(note(entry.passage.chars().take(250).collect::<String>()))
                .child(
                    row()
                        .gap(8.0)
                        .child(action(
                            s,
                            format!("copy-{}", entry.id).as_str(),
                            "Copy",
                            false,
                            move |_, tx| tx.copy(answer_text.clone()),
                        ))
                        .child(action(
                            s,
                            format!("read-{}", entry.id).as_str(),
                            "Read answer",
                            false,
                            move |s, _| s.library_selected = Some(selected.clone()),
                        ))
                        .child(action(
                            s,
                            format!("remove-{}", entry.id).as_str(),
                            "Remove",
                            false,
                            move |s, _| s.remove_entry(bookmark, id.clone()),
                        )),
                ),
        );
    }
    if pages > 1 {
        content = content.child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    action(s, "library-previous", "Previous", false, |s, _| {
                        s.library_page = s.library_page.saturating_sub(1)
                    })
                    .disabled(page == 0),
                )
                .child(note(format!("Page {} of {pages}", page + 1)))
                .child(
                    action(s, "library-next", "Next", false, move |s, _| {
                        s.library_page = s.library_page.saturating_add(1).min(pages - 1);
                    })
                    .disabled(page + 1 >= pages),
                ),
        );
    }
    if let Some(notice) = &s.notice {
        content = content.child(note(notice));
    }
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

#[cfg(test)]
mod layout_regressions {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn header_drag_region_excludes_buttons_and_reader_text() {
        let mut runtime = Runtime::new(
            AppState::default(),
            view,
            (560.0, 720.0),
            Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        runtime.pointer_down(30.0, 24.0);
        assert_eq!(
            runtime.take_titlebar_action(),
            Some(rune::TitlebarAction::Drag)
        );
        assert!(runtime.take_titlebar_action().is_none());
        runtime.pointer_up(30.0, 24.0);
        for key in ["pin-reader", "open-settings", "passage-input"] {
            for _ in 0..60 {
                runtime.frame(1.0 / 60.0);
            }
            let (x, y) = runtime.find(key).unwrap();
            runtime.pointer_down(x, y);
            assert!(
                runtime.take_titlebar_action().is_none(),
                "{key} started a window drag"
            );
            runtime.pointer_up(x, y);
            runtime.frame(1.0 / 60.0);
            if key == "pin-reader" {
                assert!(runtime.state.pinned);
            }
            if key == "open-settings" {
                assert_eq!(runtime.state.route, Route::Settings);
                runtime.update(|s, _| s.route = Route::Reader);
            }
        }
    }

    #[test]
    fn retry_shortcut_is_shown_only_for_live_initial_registration_failure() {
        use crate::invocation_ui::InvocationRegistration;
        let mut runtime = Runtime::new(
            AppState::default(),
            |s, _| {
                col()
                    .w_full()
                    .h_full()
                    .padding(16.0)
                    .child(invocation_recovery(s))
            },
            (480.0, 240.0),
            Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        assert!(runtime.find("retry-invocation").is_none());
        runtime.update(|s, _| {
            s.native_enabled = true;
            s.invocation_registration = InvocationRegistration::NeedsRetry;
        });
        for _ in 0..60 {
            runtime.frame(1.0 / 60.0);
        }
        let (x, y) = runtime.find("retry-invocation").unwrap();
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0 / 60.0);
        assert!(
            runtime
                .state
                .notice
                .as_deref()
                .unwrap()
                .contains("native invocation service")
        );
        assert!(!runtime.state.settings_pending);
        runtime.update(|s, _| s.invocation_registration = InvocationRegistration::Ready);
        runtime.frame(0.0);
        assert!(runtime.find("retry-invocation").is_none());
        runtime.update(|s, _| {
            s.native_enabled = false;
            s.invocation_registration = InvocationRegistration::NeedsRetry;
        });
        runtime.frame(0.0);
        assert!(runtime.find("retry-invocation").is_none());
    }

    #[test]
    fn settings_notes_wrap_inside_the_minimum_window_width() {
        let state = AppState {
            route: Route::Settings,
            ..AppState::default()
        };
        let mut runtime = Runtime::new(state, view, (480.0, 720.0), Env::new(true, 1.0));
        runtime.frame(0.0);
        for prefix in [
            "Review a report containing",
            "A loopback SSH tunnel",
            "Checks the saved server",
            "Choose how Pickle explains",
        ] {
            let item = runtime
                .elements()
                .into_iter()
                .find(|element| {
                    element
                        .text
                        .as_deref()
                        .is_some_and(|value| value.starts_with(prefix))
                })
                .unwrap_or_else(|| panic!("Missing explanatory text {prefix}"));
            assert!(
                item.rect[0] >= 20.0 && item.rect[0] + item.rect[2] <= 460.0,
                "Explanatory text overflowed: {prefix}, {:?}",
                item.rect
            );
            assert!(
                item.rect[3] >= 35.0,
                "Explanatory text did not wrap: {prefix}, {:?}",
                item.rect
            );
        }
    }

    #[test]
    fn adjustable_forest_tint_is_a_separate_child_of_the_material_host() {
        let mut runtime = Runtime::new(
            AppState::default(),
            view,
            (480.0, 720.0),
            Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        for opacity in [1.0, 0.4] {
            runtime.update(|state, _| state.settings.background_opacity = opacity);
            for _ in 0..120 {
                runtime.frame(1.0 / 60.0);
            }
            let elements = runtime.elements();
            let tint = elements
                .iter()
                .find(|element| element.key.as_deref() == Some("pickle-window"))
                .unwrap();
            let host = &elements[tint.parent.unwrap()];
            assert_eq!(host.key.as_deref(), Some("pickle-material"));
            assert!((tint.fill.a - opacity).abs() < 0.001);
            assert!((tint.fill.r - PAPER.r).abs() < 0.00001);
            assert!((tint.fill.g - PAPER.g).abs() < 0.00001);
            assert!((tint.fill.b - PAPER.b).abs() < 0.00001);
            assert_eq!(host.fill.a, 0.0);
            assert!((tint.rect[2] - 480.0).abs() < 0.1 && (tint.rect[3] - 720.0).abs() < 0.1);
        }
    }

    #[test]
    fn expanded_source_remains_readable_at_the_minimum_width() {
        let mut state = AppState::default();
        state.sample_result();
        state.source_expanded = true;
        let mut runtime = Runtime::new(state, view, (480.0, 720.0), Env::new(true, 1.0));
        runtime.frame(0.0);
        let item = runtime
            .elements()
            .into_iter()
            .find(|element| element.text.as_deref() == Some(crate::controller::SAMPLE))
            .unwrap();
        assert!(item.rect[2] <= 410.0 && item.rect[0] + item.rect[2] <= 460.0);
        assert!(item.rect[3] > 42.0);
    }
}

#[cfg(test)]
mod follow_up_regressions {
    use super::*;
    use rune::{Env, Key, KeyEvent, Mods, Runtime};

    fn reader() -> Runtime<AppState> {
        let mut state = AppState::default();
        state.sample_result();
        state.demo = false;
        let mut runtime = Runtime::new(state, view, (560.0, 1100.0), Env::new(true, 1.0));
        runtime.frame(0.0);
        runtime
    }

    fn click(runtime: &mut Runtime<AppState>, key: &str) {
        for _ in 0..60 {
            runtime.frame(1.0 / 60.0);
        }
        let (x, y) = runtime.find(key).unwrap_or_else(|| panic!("Missing {key}"));
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
    }

    fn enter(runtime: &mut Runtime<AppState>) {
        runtime.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        runtime.frame(0.0);
    }

    #[test]
    fn long_follow_up_questions_and_labels_fit_the_minimum_reader_width() {
        for (question, label) in [
            ("What uncertainty remains? ".repeat(75), None),
            ("q".repeat(pickle_core::limits::QUESTION), None),
            ("Explain the selected term.".into(), Some("x".repeat(2000))),
        ] {
            assert!(question.len() <= pickle_core::limits::QUESTION);
            let shown = label.as_ref().unwrap_or(&question).clone();
            let mut state = AppState::default();
            state.sample_result();
            state.conversation.push(pickle_core::ConversationTurn {
                question,
                label,
                answer: "A fixed, bounded answer.".into(),
                quality: pickle_core::QualityStatus::NotChecked("Fixture".into()),
            });
            let mut runtime = Runtime::new(state, view, (480.0, 720.0), Env::new(true, 1.0));
            runtime.frame(0.0);
            let item = runtime
                .elements()
                .into_iter()
                .find(|element| element.text.as_deref() == Some(&shown))
                .unwrap();
            assert!(
                item.rect[0] >= 20.0 && item.rect[0] + item.rect[2] <= 460.0,
                "Follow-up text overflowed: {:?}",
                item.rect
            );
            assert!(
                item.rect[3] > 30.0,
                "Long follow-up did not wrap: {:?}",
                item.rect
            );
            for key in [
                "copy-answer",
                "save-answer",
                "retry",
                "shorter",
                "more-detail",
                "example",
                "send-follow-up",
            ] {
                let (rect, _) = runtime.presented(key).unwrap();
                assert!(
                    rect[0] >= 20.0 && rect[0] + rect[2] <= 460.0,
                    "{key} overflowed: {rect:?}"
                );
            }
        }
    }

    #[test]
    fn follow_up_enter_and_ask_preserve_the_reading_and_request_the_same_consent() {
        let question = "What does the passage leave uncertain?";
        let mut pending_fields = None;
        for submit_with_enter in [true, false] {
            let mut runtime = reader();
            let snapshot = runtime.state.snapshot.clone();
            let result = serde_json::to_value(&runtime.state.result).unwrap();
            click(&mut runtime, "follow-up");
            runtime.paste(question);
            if submit_with_enter {
                enter(&mut runtime);
            } else {
                click(&mut runtime, "send-follow-up");
            }
            let Activity::Consent(pending) = &runtime.state.activity else {
                panic!("A valid follow-up must request destination consent");
            };
            assert_eq!(pending.action, ReadingAction::FollowUp);
            assert_eq!(pending.question, question);
            let fields = (
                pending.action,
                pending.question.clone(),
                pending.label.clone(),
                pending.chart,
                pending.limited,
            );
            if let Some(expected) = &pending_fields {
                assert_eq!(&fields, expected);
            } else {
                pending_fields = Some(fields);
            }
            assert_eq!(runtime.state.snapshot, snapshot);
            assert_eq!(serde_json::to_value(&runtime.state.result).unwrap(), result);
            assert_eq!(runtime.state.question.text, question);
            assert!(runtime.state.conversation.is_empty());
            assert!(runtime.state.entries.is_empty());
            assert!(runtime.state.services.is_none());
            assert!(!runtime.state.native_enabled);
        }
    }

    #[test]
    fn whitespace_follow_up_enter_does_not_start_or_replace_a_reading() {
        let mut runtime = reader();
        let snapshot = runtime.state.snapshot.clone();
        let result = serde_json::to_value(&runtime.state.result).unwrap();
        let revision = runtime.state.revision;
        click(&mut runtime, "follow-up");
        runtime.paste("   ");
        enter(&mut runtime);
        assert!(matches!(runtime.state.activity, Activity::Idle));
        assert!(runtime.state.last.is_none());
        assert_eq!(runtime.state.revision, revision);
        assert_eq!(runtime.state.snapshot, snapshot);
        assert_eq!(serde_json::to_value(&runtime.state.result).unwrap(), result);
        assert!(runtime.state.services.is_none());
    }

    #[test]
    fn settings_enter_does_not_submit_an_existing_follow_up_question() {
        let mut runtime = reader();
        let snapshot = runtime.state.snapshot.clone();
        runtime.update(|state, _| {
            state.question = TextEdit::new("This question must stay unsent.");
            state.route = Route::Settings;
            state.local_model = TextEdit::new("");
        });
        click(&mut runtime, "local-model");
        runtime.paste("fixture-model");
        enter(&mut runtime);
        assert_eq!(runtime.state.local_model.text, "fixture-model");
        assert_eq!(
            runtime.state.question.text,
            "This question must stay unsent."
        );
        assert_eq!(runtime.state.route, Route::Settings);
        assert!(matches!(runtime.state.activity, Activity::Idle));
        assert!(runtime.state.last.is_none());
        assert_eq!(runtime.state.snapshot, snapshot);
        assert!(runtime.state.services.is_none());
    }

    #[test]
    fn follow_up_enter_keeps_the_controller_pause_and_pending_settings_gates() {
        for paused in [true, false] {
            let mut runtime = reader();
            runtime.update(|state, _| {
                state.paused = paused;
                state.settings_pending = !paused;
            });
            click(&mut runtime, "follow-up");
            runtime.paste("What does this mean?");
            enter(&mut runtime);
            if paused {
                assert!(
                    matches!(&runtime.state.activity, Activity::Failed(error) if error.contains("paused"))
                );
            } else {
                assert!(matches!(runtime.state.activity, Activity::Idle));
                assert!(runtime.state.last.is_none());
                assert!(
                    runtime
                        .state
                        .notice
                        .as_deref()
                        .unwrap()
                        .contains("finish saving")
                );
            }
            assert!(runtime.state.services.is_none());
            assert_eq!(
                runtime.state.snapshot.as_ref().unwrap().text,
                crate::controller::SAMPLE
            );
        }
    }

    #[test]
    fn a_stale_follow_up_submit_cannot_cancel_a_busy_request() {
        let mut runtime = reader();
        runtime.update(|state, _| state.question = TextEdit::new("An unsent question."));
        let cancel = tokio_util::sync::CancellationToken::new();
        runtime.update(|state, tx| {
            state.activity = Activity::Running {
                id: state.revision,
                cancel: cancel.clone(),
                progress: "Writing".into(),
            };
            // Exercise the shared callback even if a native submit was already
            // queued when the reader removed the field and Ask button.
            submit_follow_up(state, tx);
        });
        assert!(!cancel.is_cancelled());
        assert!(matches!(runtime.state.activity, Activity::Running { .. }));
        assert!(runtime.state.last.is_none());
        runtime.update(|state, tx| {
            state.activity = Activity::WaitingContext;
            submit_follow_up(state, tx);
        });
        assert!(matches!(runtime.state.activity, Activity::WaitingContext));
        assert!(runtime.state.last.is_none());
        assert!(runtime.state.services.is_none());
    }
}
