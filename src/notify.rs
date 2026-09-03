use std::time::Duration;

use gpui::{
    App, Global, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, Styled, Timer, div, px, rgb,
};

use crate::theme;

const LIFETIME: Duration = Duration::from_secs(3);
const MAX_TOASTS: usize = 3;

#[derive(Default)]
pub struct Notifications {
    next_id: u64,
    items: Vec<Toast>,
}

impl Global for Notifications {}

#[derive(Clone)]
pub struct Toast {
    id: u64,
    message: SharedString,
}

pub fn show(cx: &mut App, message: impl Into<SharedString>) {
    let id = {
        let list = cx.default_global::<Notifications>();
        list.next_id += 1;
        let id = list.next_id;
        list.items.push(Toast {
            id,
            message: message.into(),
        });
        if list.items.len() > MAX_TOASTS {
            list.items.remove(0);
        }
        id
    };

    cx.spawn(async move |cx| {
        Timer::after(LIFETIME).await;
        let _ = cx.update(|cx| dismiss(id, cx));
    })
    .detach();
}

pub fn dismiss(id: u64, cx: &mut App) {
    cx.default_global::<Notifications>().items.retain(|toast| toast.id != id);
}

pub fn overlay(cx: &App) -> impl IntoElement {
    let items = cx
        .try_global::<Notifications>()
        .map(|list| list.items.clone())
        .unwrap_or_default();

    div()
        .absolute()
        .right_4()
        .bottom_4()
        .flex()
        .flex_col()
        .gap_2()
        .items_end()
        .children(items.into_iter().map(toast_card))
}

fn toast_card(toast: Toast) -> impl IntoElement {
    let id = toast.id;
    div()
        .id(("toast", id))
        .max_w(px(320.0))
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(theme::TOOLTIP))
        .border_1()
        .border_color(rgb(theme::SIDEBAR_BORDER))
        .shadow_md()
        .text_sm()
        .text_color(rgb(theme::TEXT))
        .cursor_pointer()
        .child(toast.message)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            dismiss(id, cx);
            cx.stop_propagation();
        })
}
