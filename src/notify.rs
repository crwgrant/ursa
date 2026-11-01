use gpui::{AnyWindowHandle, App, AppContext as _, SharedString};
use gpui::component::{notification::Notification, WindowExt as _};

pub fn show(cx: &mut App, message: impl Into<SharedString>) {
    let message = message.into();
    for handle in cx.windows() {
        push_to_window(handle, message.clone(), cx);
    }
}

fn push_to_window(handle: AnyWindowHandle, message: SharedString, cx: &mut App) {
    let _ = cx.update_window(handle, |_, window, cx| {
        window.push_notification(Notification::info(message), cx);
    });
}
