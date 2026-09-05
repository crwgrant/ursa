#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod cwd;
mod frame;
mod input;
mod notify;
mod panes;
mod pty;
mod session;
mod settings;
mod theme;

use std::collections::HashMap;

use gpui::{
    Action, AnyElement, AnyView, App, Application, Bounds, Context, CursorStyle, DragMoveEvent, KeyBinding, KeyDownEvent,
    Keystroke, Menu, MenuItem, MouseButton, MouseDownEvent, Pixels, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, canvas, div, point, prelude::*, px, rgb, size,
};
use panes::{PaneSpec, SplitAxis};
use session::{Session, SessionEvent, TabRestore};

actions!(
    ursa,
    [
        Quit,
        NewWindow,
        NewSession,
        NewTab,
        CloseTab,
        CloseSession,
        Copy,
        Paste,
        ClearScreen,
        OpenSettings,
        SplitRight,
        SplitDown,
        FocusNextPane,
        FocusPrevPane
    ]
);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = ursa, no_json)]
struct ActivateTab {
    index: usize,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = ursa, no_json)]
struct ActivateSession {
    index: usize,
}

pub(crate) const APP_ID: &str = "com.crwgrant.ursa";
const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 720.0;
const NEW_WINDOW_OFFSET: f32 = 28.0;
const PANE_MIN: f32 = 80.0;

enum PaneNode {
    Leaf {
        session: gpui::Entity<Session>,
    },
    Split {
        id: u64,
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

struct Tab {
    root: PaneNode,
    focused: usize,
}

struct SessionGroup {
    tabs: Vec<Tab>,
    active: usize,
}

struct Workspace {
    sessions: Vec<SessionGroup>,
    active_session: usize,
    sidebar_width: f32,
    sidebar_split_locked: bool,
    dragging_tab: Option<usize>,
    tab_insert_at: Option<usize>,
    next_split_id: u64,
    pane_bounds: HashMap<u64, Bounds<Pixels>>,
    pane_split_locked: bool,
}

#[derive(Clone)]
struct TabDrag {
    index: usize,
    title: SharedString,
}

struct TabDragPreview {
    title: SharedString,
    background: u32,
    text: u32,
    accent: u32,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let background = self.background;
        let text = self.text;
        let accent = self.accent;
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(background))
            .shadow_md()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(accent)))
            .child(div().text_sm().text_color(rgb(text)).child(self.title.clone()))
    }
}

#[derive(Clone)]
struct SidebarSplit;

struct SplitDragPreview;

impl Render for SplitDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(Clone)]
struct PaneSplit {
    id: u64,
}

impl PaneNode {
    fn leaf(session: gpui::Entity<Session>) -> Self {
        Self::Leaf { session }
    }

    fn spec(&self) -> PaneSpec {
        match self {
            Self::Leaf { .. } => PaneSpec::Leaf,
            Self::Split {
                axis,
                ratio,
                first,
                second,
                ..
            } => PaneSpec::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first.spec()),
                second: Box::new(second.spec()),
            },
        }
    }

    fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    fn leaves(&self) -> Vec<&gpui::Entity<Session>> {
        match self {
            Self::Leaf { session } => vec![session],
            Self::Split { first, second, .. } => {
                let mut leaves = first.leaves();
                leaves.extend(second.leaves());
                leaves
            }
        }
    }

    fn leaf_at(&self, index: usize) -> Option<&gpui::Entity<Session>> {
        self.leaves().into_iter().nth(index)
    }

    fn find(&self, session: &gpui::Entity<Session>) -> Option<usize> {
        self.leaves().iter().position(|leaf| *leaf == session)
    }

    fn assign_ids(&mut self, next: &mut u64) {
        if let Self::Split { id, first, second, .. } = self {
            *id = *next;
            *next += 1;
            first.assign_ids(next);
            second.assign_ids(next);
        }
    }

    fn split_axis(&self, id: u64) -> Option<SplitAxis> {
        match self {
            Self::Leaf { .. } => None,
            Self::Split {
                id: split_id,
                axis,
                first,
                second,
                ..
            } => {
                if *split_id == id {
                    Some(*axis)
                } else {
                    first.split_axis(id).or_else(|| second.split_axis(id))
                }
            }
        }
    }

    fn equalize(&mut self) {
        if let Self::Split {
            ratio, first, second, ..
        } = self
        {
            first.equalize();
            second.equalize();
            *ratio = panes::equal_split_ratio(first.leaf_count(), second.leaf_count());
        }
    }

    fn set_ratio(&mut self, id: u64, ratio: f32) -> bool {
        match self {
            Self::Leaf { .. } => false,
            Self::Split {
                id: split_id,
                ratio: current,
                first,
                second,
                ..
            } => {
                if *split_id == id {
                    *current = panes::clamp_ratio(ratio);
                    true
                } else {
                    first.set_ratio(id, ratio) || second.set_ratio(id, ratio)
                }
            }
        }
    }

    fn split_leaf(&mut self, index: usize, axis: SplitAxis, session: gpui::Entity<Session>, id: u64) -> Option<usize> {
        match self {
            Self::Leaf { session: existing } if index == 0 => {
                let first = existing.clone();
                *self = Self::Split {
                    id,
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::leaf(first)),
                    second: Box::new(Self::leaf(session)),
                };
                Some(1)
            }
            Self::Split { first, second, .. } => {
                let left = first.leaf_count();
                if index < left {
                    first.split_leaf(index, axis, session, id)
                } else {
                    second
                        .split_leaf(index - left, axis, session, id)
                        .map(|focused| left + focused)
                }
            }
            Self::Leaf { .. } => None,
        }
    }

    fn remove_leaf(self, index: usize) -> Result<Self, Self> {
        match self {
            Self::Leaf { .. } => Err(self),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                let left = first.leaf_count();
                if index < left {
                    match first.remove_leaf(index) {
                        Ok(first) => Ok(Self::Split {
                            id,
                            axis,
                            ratio,
                            first: Box::new(first),
                            second,
                        }),
                        Err(_) => Ok(*second),
                    }
                } else {
                    match second.remove_leaf(index - left) {
                        Ok(second) => Ok(Self::Split {
                            id,
                            axis,
                            ratio,
                            first,
                            second: Box::new(second),
                        }),
                        Err(_) => Ok(*first),
                    }
                }
            }
        }
    }
}

impl Tab {
    fn from_leaf(session: gpui::Entity<Session>) -> Self {
        Self {
            root: PaneNode::leaf(session),
            focused: 0,
        }
    }

    fn title(&self, cx: &App) -> SharedString {
        self.focused_session()
            .map(|session| SharedString::from(session.read(cx).title.clone()))
            .unwrap_or_else(|| SharedString::from("Tab"))
    }

    fn focused_session(&self) -> Option<&gpui::Entity<Session>> {
        self.root.leaf_at(self.focused)
    }

    fn used(&self, cx: &App) -> bool {
        self.root.leaves().iter().any(|session| session.read(cx).used)
    }
}

impl SessionGroup {
    fn spawn(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let session = cx.new(|cx| Session::spawn(0, window, cx, TabRestore::default()));
        Self {
            tabs: vec![Tab::from_leaf(session)],
            active: 0,
        }
    }

    fn spawn_tabs(
        count: usize,
        active: usize,
        restores: Vec<config::TabLayout>,
        snapshot_session: usize,
        load_snapshots: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let count = count.max(1);
        let tabs = (0..count)
            .map(|index| {
                let layout = restores.get(index).cloned().unwrap_or_default();
                Tab::from_layout(index, layout, snapshot_session, load_snapshots, window, cx)
            })
            .collect();
        Self {
            tabs,
            active: active.min(count - 1),
        }
    }

    fn title(&self, cx: &App) -> SharedString {
        self.tabs
            .get(self.active)
            .map(|tab| tab.title(cx))
            .unwrap_or_else(|| SharedString::from("Session"))
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn focused_session(&self) -> Option<&gpui::Entity<Session>> {
        self.active_tab().and_then(Tab::focused_session)
    }
}

impl Tab {
    fn from_layout(
        index: usize,
        layout: config::TabLayout,
        snapshot_session: usize,
        load_snapshots: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let mut restores = layout.cwds.into_iter();
        let mut pane = 0;
        let root = spawn_pane_spec(
            &layout.spec,
            index,
            snapshot_session,
            load_snapshots,
            &mut pane,
            &mut restores,
            window,
            cx,
        );
        let focused = layout.focused.min(root.leaf_count().saturating_sub(1));
        Self { root, focused }
    }
}

fn spawn_pane_spec(
    spec: &PaneSpec,
    tab_index: usize,
    snapshot_session: usize,
    load_snapshots: bool,
    pane: &mut usize,
    restores: &mut impl Iterator<Item = Option<std::path::PathBuf>>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> PaneNode {
    match spec {
        PaneSpec::Leaf => {
            let pane_index = *pane;
            *pane += 1;
            let restore = TabRestore {
                cwd: restores.next().flatten(),
                snapshot: load_snapshots
                    .then(|| config::read_pane_snapshot(snapshot_session, tab_index, pane_index))
                    .flatten(),
            };
            PaneNode::leaf(cx.new(|cx| Session::spawn(tab_index, window, cx, restore)))
        }
        PaneSpec::Split {
            axis,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            id: 0,
            axis: *axis,
            ratio: *ratio,
            first: Box::new(spawn_pane_spec(
                first,
                tab_index,
                snapshot_session,
                load_snapshots,
                pane,
                restores,
                window,
                cx,
            )),
            second: Box::new(spawn_pane_spec(
                second,
                tab_index,
                snapshot_session,
                load_snapshots,
                pane,
                restores,
                window,
                cx,
            )),
        },
    }
}

impl Workspace {
    fn new(restore: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let layout = if restore {
            config::restored_workspace_layout(cx)
        } else {
            config::WorkspaceLayout::default()
        };
        let snapshot_session = if config::sessions_enabled(cx) {
            None
        } else {
            Some(layout.active_session)
        };
        let layout = if config::sessions_enabled(cx) {
            layout
        } else {
            layout.into_single_session()
        };
        let sessions = layout
            .sessions
            .iter()
            .enumerate()
            .map(|(session_index, spec)| {
                let snapshot_session = snapshot_session.unwrap_or(session_index);
                SessionGroup::spawn_tabs(spec.tabs, spec.active, spec.tab_specs.clone(), snapshot_session, restore, window, cx)
            })
            .collect::<Vec<_>>();
        let mut workspace = Self {
            sessions,
            active_session: layout.active_session,
            sidebar_width: config::restored_sidebar_width(),
            sidebar_split_locked: false,
            dragging_tab: None,
            tab_insert_at: None,
            next_split_id: 0,
            pane_bounds: HashMap::new(),
            pane_split_locked: false,
        };
        workspace.assign_all_split_ids();
        for index in 0..workspace.sessions.len() {
            workspace.subscribe_group(index, window, cx);
        }
        cx.observe_global::<notify::Notifications>(|_, cx| cx.notify()).detach();
        cx.observe_global::<config::AppSettings>(|this, cx| {
            this.apply_config(cx);
            this.sync_session_sidebar(cx);
            apply_app_menus(cx);
            if config::persist_sessions(cx) {
                this.persist_layout(cx);
            } else {
                config::discard_workspace_layout();
            }
            cx.notify();
        })
        .detach();
        cx.observe_global::<theme::ThemeCatalog>(|this, cx| {
            this.apply_config(cx);
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, |_, _, cx| cx.notify()).detach();
        cx.observe_window_bounds(window, |_, window, cx| {
            config::save_window_state(window, cx);
        })
        .detach();
        workspace
    }

    fn assign_all_split_ids(&mut self) {
        for group in &mut self.sessions {
            for tab in &mut group.tabs {
                tab.root.assign_ids(&mut self.next_split_id);
            }
        }
    }

    fn subscribe_group(&self, session: usize, window: &Window, cx: &mut Context<Self>) {
        let Some(group) = self.sessions.get(session) else {
            return;
        };
        for tab in &group.tabs {
            for leaf in tab.root.leaves() {
                self.subscribe_terminal(leaf, window, cx);
            }
        }
    }

    fn subscribe_terminal(&self, tab: &gpui::Entity<Session>, window: &Window, cx: &mut Context<Self>) {
        cx.subscribe_in(tab, window, |this, session, event: &SessionEvent, window, cx| match event {
            SessionEvent::Exited => {
                if !config::keep_tab_on_exit(cx) {
                    this.close_terminal(session, window, cx);
                }
            }
            SessionEvent::TitleChanged => cx.notify(),
            SessionEvent::CwdChanged => this.persist_layout(cx),
        })
        .detach();
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.sessions.get(self.active_session).and_then(SessionGroup::active_tab)
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let session = self.active_session;
        self.sessions
            .get_mut(session)
            .and_then(|group| group.tabs.get_mut(group.active))
    }

    fn focused_session(&self) -> Option<&gpui::Entity<Session>> {
        self.sessions.get(self.active_session).and_then(SessionGroup::focused_session)
    }

    fn add_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !config::sessions_enabled(cx) {
            return;
        }
        let index = self.sessions.len();
        let group = SessionGroup::spawn(window, cx);
        self.sessions.push(group);
        self.active_session = index;
        self.subscribe_group(index, window, cx);
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.sessions.get_mut(self.active_session) else {
            return;
        };
        let index = group.tabs.len();
        let session = cx.new(|cx| Session::spawn(index, window, cx, TabRestore::default()));
        group.tabs.push(Tab::from_leaf(session.clone()));
        group.active = index;
        self.subscribe_terminal(&session, window, cx);
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn select_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !config::sessions_enabled(cx) {
            return;
        }
        if index < self.sessions.len() {
            self.active_session = index;
            self.focus_active(window, cx);
            self.persist_layout(cx);
            cx.notify();
        }
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.sessions.get_mut(self.active_session) {
            if index < group.tabs.len() {
                group.active = index;
                self.focus_active(window, cx);
                self.persist_layout(cx);
                cx.notify();
            }
        }
    }

    fn close_terminal(&mut self, session: &gpui::Entity<Session>, window: &mut Window, cx: &mut Context<Self>) {
        for (session_index, group) in self.sessions.iter().enumerate() {
            for (tab_index, tab) in group.tabs.iter().enumerate() {
                if let Some(leaf) = tab.root.find(session) {
                    self.close_pane_leaf(session_index, tab_index, leaf, window, cx);
                    return;
                }
            }
        }
    }

    fn close_pane_leaf(
        &mut self,
        session_index: usize,
        tab_index: usize,
        leaf: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.sessions.get(session_index).and_then(|group| group.tabs.get(tab_index)) else {
            return;
        };
        if tab.root.leaf_count() <= 1 {
            self.close_pane_tab(session_index, tab_index, window, cx);
            return;
        }
        let Some(group) = self.sessions.get_mut(session_index) else {
            return;
        };
        let Some(tab) = group.tabs.get_mut(tab_index) else {
            return;
        };
        let Some(placeholder) = tab.root.leaf_at(0).cloned() else {
            return;
        };
        let root = std::mem::replace(&mut tab.root, PaneNode::leaf(placeholder));
        match root.remove_leaf(leaf) {
            Ok(root) => {
                let remaining = root.leaf_count();
                tab.root = root;
                tab.focused = focus_after_remove(tab.focused, leaf, remaining);
                self.focus_active(window, cx);
                self.persist_layout(cx);
                cx.notify();
            }
            Err(root) => {
                tab.root = root;
                self.close_pane_tab(session_index, tab_index, window, cx);
            }
        }
    }

    fn close_pane_tab(&mut self, session_index: usize, tab_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.sessions.get_mut(session_index) else {
            return;
        };
        if group.tabs.len() == 1 {
            self.close_session(session_index, window, cx);
            return;
        }
        group.tabs.remove(tab_index);
        if group.active == tab_index {
            group.active = tab_index.saturating_sub(1);
        } else if group.active > tab_index {
            group.active -= 1;
        }
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn close_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.sessions.len() {
            return;
        }
        if self.sessions.len() == 1 {
            self.persist_layout_and_flush(cx);
            window.remove_window();
            return;
        }
        self.sessions.remove(index);
        if self.active_session == index {
            self.active_session = index.saturating_sub(1);
        } else if self.active_session > index {
            self.active_session -= 1;
        }
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.active_session;
        let Some((tab, leaf)) = self.sessions.get(session).and_then(|group| {
            let tab = group.active;
            group.tabs.get(tab).map(|pane| (tab, pane.focused))
        }) else {
            return;
        };
        self.close_pane_leaf(session, tab, leaf, window, cx);
    }

    fn close_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !config::sessions_enabled(cx) {
            return;
        }
        self.close_session(self.active_session, window, cx);
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session().cloned() {
            session.update(cx, |session, _cx| session.focus(window));
        }
        self.refresh_pane_cursors(cx);
    }

    fn refresh_pane_cursors(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        for session in tab.root.leaves() {
            session.update(cx, |_, cx| cx.notify());
        }
    }

    fn focus_leaf(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        if index >= tab.root.leaf_count() {
            return;
        }
        let changed = tab.focused != index;
        tab.focused = index;
        if let Some(session) = tab.focused_session().cloned() {
            session.update(cx, |session, _| session.focus(window));
        }
        self.refresh_pane_cursors(cx);
        if changed {
            self.persist_layout(cx);
        }
        cx.notify();
    }

    fn focus_offset(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.focused = panes::wrap_focus(tab.focused, tab.root.leaf_count(), delta);
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn split_focused(&mut self, axis: SplitAxis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.root.leaf_count() >= panes::MAX_PANES {
            return;
        }
        let cwd = tab.focused_session().and_then(|session| session.read(cx).spawn_cwd());
        let tab_index = self.sessions.get(self.active_session).map(|group| group.active).unwrap_or(0);
        let session = cx.new(|cx| Session::spawn(tab_index, window, cx, TabRestore { cwd, snapshot: None }));
        let id = self.next_split_id;
        self.next_split_id += 1;
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let focused = tab.focused;
        if let Some(next) = tab.root.split_leaf(focused, axis, session.clone(), id) {
            tab.root.equalize();
            tab.focused = next;
            self.subscribe_terminal(&session, window, cx);
            self.focus_active(window, cx);
            self.persist_layout(cx);
            cx.notify();
        }
    }

    fn resize_split(&mut self, id: u64, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.pane_bounds.get(&id).copied() else {
            return;
        };
        let Some(axis) = self
            .sessions
            .iter()
            .find_map(|group| group.tabs.iter().find_map(|tab| tab.root.split_axis(id)))
        else {
            return;
        };
        let ratio = match axis {
            SplitAxis::Horizontal => {
                let width = f32::from(bounds.size.width).max(1.0);
                (f32::from(position.x) - f32::from(bounds.origin.x)) / width
            }
            SplitAxis::Vertical => {
                let height = f32::from(bounds.size.height).max(1.0);
                (f32::from(position.y) - f32::from(bounds.origin.y)) / height
            }
        };
        if self.active_tab_mut().is_some_and(|tab| tab.root.set_ratio(id, ratio)) {
            cx.notify();
        }
    }

    fn reset_split_ratio(&mut self, id: u64, cx: &mut Context<Self>) {
        self.pane_split_locked = true;
        if self.active_tab_mut().is_some_and(|tab| tab.root.set_ratio(id, 0.5)) {
            self.persist_layout(cx);
            cx.notify();
        }
    }

    fn finish_pane_resize(&mut self, cx: &mut Context<Self>) {
        self.pane_split_locked = false;
        self.persist_layout(cx);
    }

    fn can_copy(&self, cx: &App) -> bool {
        self.focused_session().is_some_and(|session| session.read(cx).has_selection())
    }

    fn copy_active(&self, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session().cloned() {
            session.update(cx, |session, cx| session.copy_selection(cx));
        }
    }

    fn paste_active(&self, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session().cloned() {
            session.update(cx, |session, cx| session.paste_clipboard(cx));
        }
    }

    fn clear_active(&self, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session().cloned() {
            session.update(cx, |session, _cx| session.clear_screen());
        }
    }

    fn forward_tab_to_focused(&self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.key != "tab" || keystroke.modifiers.platform || keystroke.modifiers.control || keystroke.modifiers.alt {
            return false;
        }
        let Some(session) = self.focused_session().cloned() else {
            return false;
        };
        session.update(cx, |session, cx| session.send_keystroke(keystroke, cx))
    }

    fn apply_config(&self, cx: &mut Context<Self>) {
        for group in &self.sessions {
            for tab in &group.tabs {
                for session in tab.root.leaves() {
                    session.update(cx, |session, cx| session.apply_config(cx));
                }
            }
        }
    }

    fn sync_session_sidebar(&mut self, cx: &mut Context<Self>) {
        if config::sessions_enabled(cx) || self.sessions.len() <= 1 {
            return;
        }
        let active = self.active_session.min(self.sessions.len().saturating_sub(1));
        let group = self.sessions.remove(active);
        self.sessions = vec![group];
        self.active_session = 0;
    }

    fn set_sidebar_width(&mut self, width: f32, window: &Window, persist: bool, cx: &mut Context<Self>) {
        let width = config::clamp_sidebar_width(width, f32::from(window.viewport_size().width));
        if (self.sidebar_width - width).abs() < 0.5 {
            if persist {
                config::save_sidebar_width(self.sidebar_width);
            }
            return;
        }
        self.sidebar_width = width;
        if persist {
            config::save_sidebar_width(width);
        }
        cx.notify();
    }

    fn reset_sidebar_width(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sidebar_split_locked = true;
        self.set_sidebar_width(theme::SIDEBAR_WIDTH, window, true, cx);
    }

    fn finish_sidebar_resize(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sidebar_split_locked = false;
        self.set_sidebar_width(self.sidebar_width, window, true, cx);
    }

    fn set_tab_insert_at(&mut self, from: usize, insert_at: usize, cx: &mut Context<Self>) {
        self.dragging_tab = Some(from);
        if self.tab_insert_at != Some(insert_at) {
            self.tab_insert_at = Some(insert_at);
            cx.notify();
        }
    }

    fn drop_tab(&mut self, from: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(insert_at) = self.tab_insert_at {
            self.reorder_tab(from, insert_at, window, cx);
        }
        self.dragging_tab = None;
        self.tab_insert_at = None;
        cx.notify();
    }

    fn reorder_tab(&mut self, from: usize, insert_at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dest) = tab_destination(from, insert_at, self.sessions.len()) else {
            return;
        };
        let session = self.sessions.remove(from);
        self.sessions.insert(dest, session);
        self.active_session = active_after_reorder(self.active_session, from, dest);
        self.focus_active(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn persist_layout(&self, cx: &mut Context<Self>) {
        self.write_layout(cx, false);
    }

    fn persist_layout_and_flush(&self, cx: &mut Context<Self>) {
        self.write_layout(cx, true);
    }

    fn write_layout(&self, cx: &mut Context<Self>, flush: bool) {
        let (tab_count, leaf_count, used) = self
            .sessions
            .first()
            .map(|group| {
                let tab = group.tabs.first();
                (
                    group.tabs.len(),
                    tab.map(|tab| tab.root.leaf_count()).unwrap_or(0),
                    tab.is_some_and(|tab| tab.used(cx)),
                )
            })
            .unwrap_or((0, 0, false));
        if !config::persist_sessions(cx) || is_fresh_workspace(self.sessions.len(), tab_count, leaf_count, used) {
            config::save_workspace_layout(config::WorkspaceLayout::default());
            config::clear_tab_snapshots();
            return;
        }
        for group in &self.sessions {
            for tab in &group.tabs {
                for session in tab.root.leaves() {
                    session.update(cx, |session, _| session.refresh_cwd());
                }
            }
        }
        let sessions = self
            .sessions
            .iter()
            .map(|group| config::SessionLayout {
                tabs: group.tabs.len(),
                active: group.active,
                tab_specs: group
                    .tabs
                    .iter()
                    .map(|tab| config::TabLayout {
                        spec: tab.root.spec(),
                        focused: tab.focused,
                        cwds: tab.root.leaves().iter().map(|session| session.read(cx).spawn_cwd()).collect(),
                    })
                    .collect(),
            })
            .collect();
        let layout = config::WorkspaceLayout::from_sessions(sessions, self.active_session);
        let layout = if config::sessions_enabled(cx) {
            layout
        } else {
            layout.into_single_session()
        };
        config::save_workspace_layout(layout.clone());
        for (session_index, group) in self.sessions.iter().enumerate() {
            for (tab_index, tab) in group.tabs.iter().enumerate() {
                for (pane_index, session) in tab.root.leaves().into_iter().enumerate() {
                    let path = config::pane_snapshot_path(session_index, tab_index, pane_index);
                    if flush {
                        session.read(cx).flush_state(path);
                    } else {
                        session.read(cx).request_save_state(path);
                    }
                }
            }
        }
        config::prune_tab_snapshots(&layout);
    }
}

fn is_fresh_workspace(session_count: usize, tab_count: usize, leaf_count: usize, tab_used: bool) -> bool {
    session_count == 1 && tab_count == 1 && leaf_count == 1 && !tab_used
}

fn focus_after_remove(focused: usize, removed: usize, remaining: usize) -> usize {
    let remaining = remaining.max(1);
    if focused == removed {
        focused.min(remaining - 1)
    } else if focused > removed {
        (focused - 1).min(remaining - 1)
    } else {
        focused.min(remaining - 1)
    }
}

fn tab_destination(from: usize, insert_at: usize, len: usize) -> Option<usize> {
    if from >= len || insert_at > len {
        return None;
    }
    if insert_at == from || insert_at == from + 1 {
        return None;
    }
    Some(if insert_at > from { insert_at - 1 } else { insert_at })
}

fn active_after_reorder(active: usize, from: usize, dest: usize) -> usize {
    if active == from {
        dest
    } else if from < active && dest >= active {
        active - 1
    } else if from > active && dest <= active {
        active + 1
    } else {
        active
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.dragging_tab = None;
            self.tab_insert_at = None;
        }
        let active = self.active_session;
        let tabs: Vec<(usize, SharedString, bool)> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(index, group)| (index, group.title(cx), index == active))
            .collect();

        let colors = theme::colors(cx);
        div()
            .relative()
            .flex()
            .size_full()
            .bg(rgb(colors.window))
            .text_color(rgb(colors.text))
            .font_family(theme::UI_FONT_FAMILY)
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.add_session(window, cx)))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.add_tab(window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| this.close_active_session(window, cx)))
            .on_action(cx.listener(|this, action: &ActivateTab, window, cx| this.select_tab(action.index, window, cx)))
            .on_action(cx.listener(|this, action: &ActivateSession, window, cx| this.select_session(action.index, window, cx)))
            .when(self.can_copy(cx), |el| {
                el.on_action(cx.listener(|this, _: &Copy, _window, cx| this.copy_active(cx)))
            })
            .when(session::clipboard_has_text(cx), |el| {
                el.on_action(cx.listener(|this, _: &Paste, _window, cx| this.paste_active(cx)))
            })
            .on_action(cx.listener(|this, _: &ClearScreen, _window, cx| this.clear_active(cx)))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.forward_tab_to_focused(&event.keystroke, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| this.split_focused(SplitAxis::Horizontal, window, cx)))
            .on_action(cx.listener(|this, _: &SplitDown, window, cx| this.split_focused(SplitAxis::Vertical, window, cx)))
            .on_action(cx.listener(|this, _: &FocusNextPane, window, cx| this.focus_offset(1, window, cx)))
            .on_action(cx.listener(|this, _: &FocusPrevPane, window, cx| this.focus_offset(-1, window, cx)))
            .on_action(|_: &OpenSettings, _window, cx| crate::settings::open(cx))
            .when(config::sessions_enabled(cx), |workspace| {
                workspace.child(self.render_sidebar(&tabs, cx)).child(self.render_split(cx))
            })
            .child(self.render_terminal(cx))
            .child(notify::overlay(cx))
    }
}

impl Workspace {
    fn render_sidebar(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .w(px(self.sidebar_width))
            .h_full()
            .bg(rgb(colors.sidebar))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("sessions-header")
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_3()
                    .when(tabs.len() > 1, |header| {
                        header
                            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<TabDrag>, _, cx| {
                                if tab_drag_is_over(event) {
                                    this.set_tab_insert_at(event.drag(cx).index, 0, cx);
                                }
                            }))
                            .on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                                this.drop_tab(drag.index, window, cx);
                            }))
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.text_dim))
                            .child("SESSIONS"),
                    )
                    .child(new_tab_button(cx)),
            )
            .child(self.render_tab_list(tabs, cx))
            .child(settings_button(cx))
    }

    fn render_tab_list(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let can_reorder = tabs.len() > 1;
        let insert_at = self.tab_insert_at;
        let dragging_tab = self.dragging_tab;
        let last = tabs.len().saturating_sub(1);
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(tab_list_edge("tab-list-start", 0, can_reorder, cx).into_any_element());
        for (index, title, selected) in tabs {
            let insert = if insert_at == Some(*index) {
                Some(TabInsert::Before)
            } else if insert_at == Some(*index + 1) && *index == last {
                Some(TabInsert::After)
            } else {
                None
            };
            items.push(
                tab_row(*index, title.clone(), *selected, dragging_tab == Some(*index), insert, can_reorder, cx)
                    .into_any_element(),
            );
        }
        items.push(tab_list_edge("tab-list-end", tabs.len(), can_reorder, cx).into_any_element());
        div()
            .id("tab-list")
            .flex()
            .flex_col()
            .flex_1()
            .px_2()
            .gap_1()
            .overflow_y_scroll()
            .children(items)
            .when(can_reorder, |list| {
                list.on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                    this.drop_tab(drag.index, window, cx);
                }))
            })
    }

    fn render_terminal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(group) = self.sessions.get(self.active_session) else {
            return div().flex_1().h_full().min_w_0();
        };
        let tabs: Vec<(usize, SharedString, bool)> = group
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| (index, tab.title(cx), index == group.active))
            .collect();
        let panes = group.active_tab().map(|tab| {
            let show_focus = tab.root.leaf_count() > 1;
            self.render_pane_node(&tab.root, tab.focused, 0, show_focus, cx)
        });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(self.render_pane_tabs(&tabs, cx))
            .when_some(panes, |pane, tree| pane.child(div().flex_1().min_h_0().min_w_0().child(tree)))
    }

    fn render_pane_node(
        &self,
        node: &PaneNode,
        focused: usize,
        offset: usize,
        show_focus: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            PaneNode::Leaf { session } => {
                let colors = theme::colors(cx);
                let is_focused = focused == offset;
                div()
                    .id(("pane-leaf", offset))
                    .size_full()
                    .min_w(px(PANE_MIN))
                    .min_h(px(PANE_MIN))
                    .overflow_hidden()
                    .when(show_focus && is_focused, |pane| pane.border_1().border_color(rgb(colors.accent)))
                    .capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
                        this.focus_leaf(offset, window, cx);
                    }))
                    .child(session.clone())
                    .into_any_element()
            }
            PaneNode::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                let id = *id;
                let colors = theme::colors(cx);
                let left = first.leaf_count();
                let first = self.render_pane_node(first, focused, offset, show_focus, cx);
                let second = self.render_pane_node(second, focused, offset + left, show_focus, cx);
                let entity = cx.entity();
                let horizontal = *axis == SplitAxis::Horizontal;
                div()
                    .id(("pane-split", id as usize))
                    .relative()
                    .flex()
                    .when(horizontal, |el| el.flex_row())
                    .when(!horizontal, |el| el.flex_col())
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                entity.update(cx, |this, _| {
                                    this.pane_bounds.insert(id, bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(split_pane_slot(*ratio, horizontal, first))
                    .child(self.render_pane_gutter(id, *axis, colors, cx))
                    .child(split_pane_slot(1.0 - *ratio, horizontal, second))
                    .into_any_element()
            }
        }
    }
}

fn split_pane_slot(grow: f32, horizontal: bool, child: AnyElement) -> impl IntoElement {
    let mut slot = div()
        .flex_1()
        .when(horizontal, |el| el.h_full())
        .when(!horizontal, |el| el.w_full())
        .min_w(px(PANE_MIN))
        .min_h(px(PANE_MIN))
        .overflow_hidden()
        .child(child);
    slot.style().flex_grow = Some(grow.max(0.01));
    slot
}

impl Workspace {
    fn render_pane_gutter(&self, id: u64, axis: SplitAxis, colors: theme::Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let horizontal = axis == SplitAxis::Horizontal;
        div()
            .relative()
            .flex_shrink_0()
            .when(horizontal, |el| el.w(px(1.0)).h_full())
            .when(!horizontal, |el| el.h(px(1.0)).w_full())
            .bg(rgb(colors.sidebar_border))
            .child(
                div()
                    .id(("pane-gutter", id as usize))
                    .absolute()
                    .when(horizontal, |el| el.top_0().bottom_0().left(px(-3.0)).w(px(7.0)))
                    .when(!horizontal, |el| el.left_0().right_0().top(px(-3.0)).h(px(7.0)))
                    .cursor(if horizontal {
                        CursorStyle::ResizeColumn
                    } else {
                        CursorStyle::ResizeRow
                    })
                    .hover(move |style| style.bg(rgb(colors.accent)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if event.click_count >= 2 {
                                this.reset_split_ratio(id, cx);
                            } else {
                                this.pane_split_locked = false;
                            }
                        }),
                    )
                    .on_drag(PaneSplit { id }, |_, _, _, cx| cx.new(|_| SplitDragPreview))
                    .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<PaneSplit>, _, cx| {
                        if this.pane_split_locked || event.drag(cx).id != id {
                            return;
                        }
                        this.resize_split(id, event.event.position, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.finish_pane_resize(cx);
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.finish_pane_resize(cx);
                        }),
                    ),
            )
    }

    fn render_pane_tabs(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .id("pane-tabs")
            .flex()
            .items_end()
            .h(px(36.0))
            .flex_shrink_0()
            .bg(rgb(colors.window))
            .overflow_x_scroll()
            .when(!config::sessions_enabled(cx), |bar| bar.child(pane_settings_button(cx)))
            .children(tabs.iter().enumerate().map(|(position, (index, title, selected))| {
                pane_tab(*index, title.clone(), *selected, position + 1 < tabs.len(), cx)
            }))
            .child(new_pane_tab_button(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(colors.window))
                    .border_b_1()
                    .border_color(rgb(colors.sidebar_border)),
            )
    }

    fn render_split(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .relative()
            .w(px(1.0))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(colors.sidebar_border))
            .child(
                div()
                    .id("sidebar-split")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-3.0))
                    .w(px(7.0))
                    .cursor(CursorStyle::ResizeColumn)
                    .hover(move |style| style.bg(rgb(colors.accent)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if event.click_count >= 2 {
                                this.reset_sidebar_width(window, cx);
                            } else {
                                this.sidebar_split_locked = false;
                            }
                        }),
                    )
                    .on_drag(SidebarSplit, |_, _, _, cx| cx.new(|_| SplitDragPreview))
                    .on_drag_move(cx.listener(|this, event: &DragMoveEvent<SidebarSplit>, window, cx| {
                        if this.sidebar_split_locked {
                            return;
                        }
                        this.set_sidebar_width(f32::from(event.event.position.x), window, false, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.finish_sidebar_resize(window, cx);
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.finish_sidebar_resize(window, cx);
                        }),
                    ),
            )
    }
}

fn settings_button(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .px_2()
        .py_2()
        .border_t_1()
        .border_color(rgb(colors.sidebar_border))
        .flex_shrink_0()
        .child(
            div()
                .id("open-settings")
                .w_full()
                .rounded_md()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(colors.tab_hover)))
                .tooltip(action_tooltip("Settings", settings_shortcut()))
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(colors.text_dim)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(rgb(colors.text_dim))
                        .child("Settings"),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    crate::settings::open(cx);
                    cx.stop_propagation();
                }),
        )
}

fn new_tab_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id("new-tab")
        .h(px(22.0))
        .w(px(22.0))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(colors.button))
        .text_color(rgb(colors.text))
        .text_sm()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
        .tooltip(action_tooltip("New Session", new_session_shortcut()))
        .child("+")
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, window, cx| this.add_session(window, cx)))
}

fn pane_settings_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id("pane-settings")
        .h_full()
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .border_b_1()
        .border_color(rgb(colors.sidebar_border))
        .text_sm()
        .text_color(rgb(colors.text_dim))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)).text_color(rgb(colors.text)))
        .tooltip(action_tooltip("Settings", settings_shortcut()))
        .child("Settings")
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            crate::settings::open(cx);
            cx.stop_propagation();
        })
}

fn new_pane_tab_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id("new-pane-tab")
        .h_full()
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .border_b_1()
        .border_color(rgb(colors.sidebar_border))
        .text_sm()
        .text_color(rgb(colors.text_dim))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)).text_color(rgb(colors.text)))
        .tooltip(action_tooltip("New Tab", new_tab_shortcut()))
        .child("+")
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, window, cx| this.add_tab(window, cx)))
}

fn pane_tab(
    index: usize,
    title: SharedString,
    selected: bool,
    show_divider: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    let background = if selected { colors.term_bg } else { colors.window };
    let accent = colors.accent;
    let text = if selected { colors.text } else { colors.text_dim };
    div()
        .id(("pane-tab", index))
        .relative()
        .h_full()
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .min_w(px(140.0))
        .max_w(px(220.0))
        .bg(rgb(background))
        .cursor_pointer()
        .when(!selected, |tab| tab.border_b_1().border_color(rgb(colors.sidebar_border)))
        .when(show_divider && !selected, |tab| tab.border_r_1().border_color(rgb(colors.sidebar_border)))
        .hover(move |style| if selected { style } else { style.bg(rgb(colors.tab_hover)) })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| this.select_tab(index, window, cx)),
        )
        .child(
            div()
                .w(px(14.0))
                .h(px(14.0))
                .rounded_sm()
                .border_1()
                .border_color(rgb(colors.sidebar_border))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(colors.text_dim))
                .child(">_"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_ellipsis()
                .text_color(rgb(text))
                .child(title),
        )
        .when(selected, |tab| {
            tab.child(close_pane_tab_button(index, cx))
                .child(div().absolute().top_0().left_0().right_0().h(px(2.0)).bg(rgb(accent)))
        })
}

fn close_pane_tab_button(index: usize, cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id(("pane-tab-close", index))
        .h(px(16.0))
        .w(px(16.0))
        .rounded_sm()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_xs()
        .text_color(rgb(colors.text_dim))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.button)).text_color(rgb(colors.text)))
        .tooltip(action_tooltip("Close Tab", close_tab_shortcut()))
        .child("×")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| {
                cx.stop_propagation();
                let session = this.active_session;
                this.close_pane_tab(session, index, window, cx);
            }),
        )
}

fn close_tab_button(index: usize, cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id(("tab-close", index))
        .h(px(18.0))
        .w(px(18.0))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_xs()
        .text_color(rgb(colors.text_dim))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.button)).text_color(rgb(colors.text)))
        .tooltip(action_tooltip("Close Session", close_session_shortcut()))
        .child("×")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| {
                cx.stop_propagation();
                this.close_session(index, window, cx);
            }),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabInsert {
    Before,
    After,
}

fn tab_drag_is_over(event: &DragMoveEvent<TabDrag>) -> bool {
    event.bounds.contains(&event.event.position)
}

fn tab_insert_line(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div().w_full().h(px(2.0)).rounded_full().bg(rgb(colors.accent))
}

fn tab_list_edge(id: &'static str, insert_at: usize, can_reorder: bool, cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id(id)
        .when(id == "tab-list-end", |edge| edge.flex_1().min_h(px(12.0)))
        .when(id == "tab-list-start", |edge| edge.h(px(8.0)))
        .when(can_reorder, |edge| {
            edge.on_drag_move(cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                if tab_drag_is_over(event) {
                    this.set_tab_insert_at(event.drag(cx).index, insert_at, cx);
                }
            }))
            .on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                this.drop_tab(drag.index, window, cx);
            }))
        })
}

fn tab_row(
    index: usize,
    title: SharedString,
    selected: bool,
    dragging: bool,
    insert: Option<TabInsert>,
    can_reorder: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    let background = if selected {
        rgb(colors.tab_active)
    } else {
        rgb(colors.sidebar)
    };
    let drag = TabDrag {
        index,
        title: title.clone(),
    };
    let on_select = cx.listener(move |this, _event, window, cx| this.select_session(index, window, cx));
    let on_drag_move = cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
        if !tab_drag_is_over(event) {
            return;
        }
        let y = event.event.position.y - event.bounds.origin.y;
        let insert_at = if y < event.bounds.size.height * 0.5 {
            index
        } else {
            index + 1
        };
        this.set_tab_insert_at(event.drag(cx).index, insert_at, cx);
    });
    let on_drop = cx.listener(move |this, drag: &TabDrag, window, cx| {
        this.drop_tab(drag.index, window, cx);
    });
    let close = close_tab_button(index, cx);

    div()
        .id(("tab", index))
        .relative()
        .w_full()
        .rounded_md()
        .px_3()
        .py_2()
        .bg(background)
        .when(dragging, |row| row.opacity(0.4))
        .when(can_reorder, |row| row.cursor_move())
        .when(!can_reorder, |row| row.cursor_pointer())
        .hover(move |style| if selected { style } else { style.bg(rgb(colors.tab_hover)) })
        .on_mouse_down(MouseButton::Left, on_select)
        .when(can_reorder, |row| {
            row.on_drag(drag, |drag, _, _, cx| {
                let colors = theme::colors(cx);
                cx.new(|_| TabDragPreview {
                    title: drag.title.clone(),
                    background: colors.tab_active,
                    text: colors.text,
                    accent: colors.accent,
                })
            })
            .on_drag_move(on_drag_move)
            .on_drop(on_drop)
        })
        .when_some(insert, |row, edge| {
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .when(edge == TabInsert::Before, |line| line.top_0())
                    .when(edge == TabInsert::After, |line| line.bottom_0())
                    .child(tab_insert_line(cx)),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(if selected {
                    rgb(colors.accent)
                } else {
                    rgb(colors.text_dim)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(if selected { rgb(colors.text) } else { rgb(colors.text_dim) })
                        .child(title),
                )
                .child(close),
        )
}

struct ActionTooltip {
    label: SharedString,
    shortcut: SharedString,
}

fn action_tooltip(
    label: impl Into<SharedString>,
    shortcut: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let label = label.into();
    let shortcut = shortcut.into();
    move |_, cx| {
        cx.new(|_| ActionTooltip {
            label: label.clone(),
            shortcut: shortcut.clone(),
        })
        .into()
    }
}

impl Render for ActionTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(colors.tooltip))
            .border_1()
            .border_color(rgb(colors.sidebar_border))
            .shadow_md()
            .text_xs()
            .child(div().text_color(rgb(colors.text)).child(self.label.clone()))
            .child(div().text_color(rgb(colors.text_dim)).child(self.shortcut.clone()))
    }
}

fn new_session_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘⇧T" } else { "Ctrl+Alt+T" }
}

fn new_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘T" } else { "Ctrl+Shift+T" }
}

fn close_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘W" } else { "Ctrl+Shift+W" }
}

fn close_session_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘⇧W" } else { "Ctrl+Alt+W" }
}

fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘," } else { "Ctrl+," }
}

fn view_menu(sessions_enabled: bool) -> Menu {
    let mut items = vec![
        MenuItem::action("Clear Screen", ClearScreen),
        MenuItem::separator(),
        MenuItem::action("Focus Next Pane", FocusNextPane),
        MenuItem::action("Focus Previous Pane", FocusPrevPane),
        MenuItem::separator(),
    ];
    if sessions_enabled {
        items.push(MenuItem::action("Close Session", CloseSession));
        items.push(MenuItem::separator());
    }
    for number in 1..=9 {
        items.push(MenuItem::action(format!("Tab {number}"), ActivateTab { index: number - 1 }));
    }
    if sessions_enabled {
        items.push(MenuItem::separator());
        for number in 1..=9 {
            items.push(MenuItem::action(format!("Session {number}"), ActivateSession { index: number - 1 }));
        }
    }
    Menu {
        name: "View".into(),
        items,
    }
}

fn file_menu(sessions_enabled: bool) -> Menu {
    let mut items = vec![MenuItem::action("New Window", NewWindow)];
    if sessions_enabled {
        items.push(MenuItem::action("New Session", NewSession));
    }
    items.push(MenuItem::action("New Tab", NewTab));
    items.push(MenuItem::action("Split Right", SplitRight));
    items.push(MenuItem::action("Split Down", SplitDown));
    items.push(MenuItem::separator());
    items.push(MenuItem::action("Close Tab", CloseTab));
    if sessions_enabled {
        items.push(MenuItem::action("Close Session", CloseSession));
    }
    Menu {
        name: "File".into(),
        items,
    }
}

fn app_menus(sessions_enabled: bool) -> Vec<Menu> {
    vec![
        Menu {
            name: "Ursa".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Ursa", Quit),
            ],
        },
        file_menu(sessions_enabled),
        Menu {
            name: "Edit".into(),
            items: vec![MenuItem::action("Copy", Copy), MenuItem::action("Paste", Paste)],
        },
        view_menu(sessions_enabled),
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Close", CloseTab)],
        },
    ]
}

fn apply_app_menus(cx: &mut App) {
    cx.set_menus(app_menus(config::sessions_enabled(cx)));
}

fn workspace_key_bindings() -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-n", NewWindow, None),
        KeyBinding::new("cmd-shift-t", NewSession, None),
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-w", CloseSession, None),
        KeyBinding::new("cmd-c", Copy, None),
        KeyBinding::new("cmd-v", Paste, None),
        KeyBinding::new("cmd-k", ClearScreen, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("ctrl-alt-t", NewSession, None),
        KeyBinding::new("ctrl-alt-w", CloseSession, None),
        KeyBinding::new("ctrl-shift-t", NewTab, None),
        KeyBinding::new("ctrl-shift-w", CloseTab, None),
        KeyBinding::new("ctrl-shift-c", Copy, None),
        KeyBinding::new("ctrl-shift-v", Paste, None),
        KeyBinding::new("cmd-d", SplitRight, None),
        KeyBinding::new("cmd-shift-d", SplitDown, None),
        KeyBinding::new("cmd-]", FocusNextPane, None),
        KeyBinding::new("cmd-[", FocusPrevPane, None),
        KeyBinding::new("ctrl-shift-d", SplitRight, None),
        KeyBinding::new("ctrl-alt-d", SplitDown, None),
        KeyBinding::new("ctrl-shift-]", FocusNextPane, None),
        KeyBinding::new("ctrl-shift-[", FocusPrevPane, None),
    ];
    for number in 1..=9 {
        let index = number - 1;
        bindings.push(KeyBinding::new(&format!("cmd-{number}"), ActivateTab { index }, None));
        bindings.push(KeyBinding::new(&format!("ctrl-shift-{number}"), ActivateTab { index }, None));
        bindings.push(KeyBinding::new(&format!("ctrl-{number}"), ActivateSession { index }, None));
    }
    bindings.push(KeyBinding::new("cmd-0", ActivateTab { index: 9 }, None));
    bindings.push(KeyBinding::new("ctrl-shift-0", ActivateTab { index: 9 }, None));
    bindings.push(KeyBinding::new("ctrl-0", ActivateSession { index: 9 }, None));
    bindings
}

fn main() {
    let app = Application::new();
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_workspace_window(cx);
        }
    });
    app.run(|cx: &mut App| {
        config::init(cx);
        cx.on_action(|_: &Quit, cx| {
            save_workspace_windows(cx);
            cx.quit();
        });
        cx.on_action(|_: &NewWindow, cx| {
            open_workspace_window(cx);
        });
        cx.on_action(|_: &OpenSettings, cx| settings::open(cx));
        cx.bind_keys(workspace_key_bindings());
        apply_app_menus(cx);
        cx.set_dock_menu(vec![MenuItem::action("New Window", NewWindow)]);

        open_workspace_window(cx);
        cx.activate(true);
    });
}

fn open_workspace_window(cx: &mut App) {
    let _ = cx.open_window(workspace_window_options(cx), |window, cx| {
        window.on_window_should_close(cx, |window, cx| {
            config::save_window_state(window, cx);
            true
        });
        let restore = cx.windows().len() <= 1;
        cx.new(|cx| Workspace::new(restore, window, cx))
    });
}

pub(crate) fn reset_workspace_sidebars(cx: &mut App) {
    for handle in cx.windows() {
        let Some(workspace) = handle.downcast::<Workspace>() else {
            continue;
        };
        let _ = workspace.update(cx, |this, window, cx| {
            this.reset_sidebar_width(window, cx);
        });
    }
}

fn save_workspace_windows(cx: &mut App) {
    for handle in cx.windows() {
        let Some(workspace) = handle.downcast::<Workspace>() else {
            continue;
        };
        let _ = workspace.update(cx, |this, window, cx| {
            this.persist_layout_and_flush(cx);
            config::save_window_state(window, cx);
        });
    }
}

fn workspace_window_options(cx: &App) -> WindowOptions {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let (mut window_bounds, display_id) =
        config::restored_window(cx).unwrap_or_else(|| (WindowBounds::Windowed(Bounds::centered(None, window_size, cx)), None));
    let stagger = cx.windows().len() as f32;
    if stagger > 0.0 {
        let offset = point(px(stagger * NEW_WINDOW_OFFSET), px(stagger * NEW_WINDOW_OFFSET));
        match &mut window_bounds {
            WindowBounds::Windowed(bounds) | WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => {
                bounds.origin = bounds.origin + offset;
            }
        }
    }

    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("Ursa".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: true,
        display_id,
        window_min_size: Some(size(px(config::WINDOW_MIN_WIDTH), px(config::WINDOW_MIN_HEIGHT))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ursa".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{active_after_reorder, focus_after_remove, is_fresh_workspace, tab_destination};

    #[test]
    fn tab_destination_skips_same_slot() {
        assert_eq!(tab_destination(1, 1, 4), None);
        assert_eq!(tab_destination(1, 2, 4), None);
        assert_eq!(tab_destination(0, 0, 1), None);
        assert_eq!(tab_destination(3, 5, 4), None);
    }

    #[test]
    fn tab_destination_moves_around_neighbors() {
        assert_eq!(tab_destination(1, 0, 4), Some(0));
        assert_eq!(tab_destination(1, 3, 4), Some(2));
        assert_eq!(tab_destination(1, 4, 4), Some(3));
        assert_eq!(tab_destination(0, 4, 4), Some(3));
        assert_eq!(tab_destination(3, 0, 4), Some(0));
    }

    #[test]
    fn active_index_follows_moved_tab() {
        assert_eq!(active_after_reorder(1, 1, 3), 3);
        assert_eq!(active_after_reorder(2, 0, 2), 1);
        assert_eq!(active_after_reorder(0, 2, 0), 1);
        assert_eq!(active_after_reorder(1, 0, 2), 0);
        assert_eq!(active_after_reorder(1, 3, 0), 2);
        assert_eq!(active_after_reorder(1, 0, 3), 0);
    }

    #[test]
    fn unused_single_session_is_fresh() {
        assert!(is_fresh_workspace(1, 1, 1, false));
        assert!(!is_fresh_workspace(1, 1, 1, true));
        assert!(!is_fresh_workspace(1, 1, 2, false));
        assert!(!is_fresh_workspace(1, 2, 1, false));
        assert!(!is_fresh_workspace(2, 1, 1, false));
        assert!(!is_fresh_workspace(0, 0, 0, false));
    }

    #[test]
    fn focus_moves_after_removing_a_pane() {
        assert_eq!(focus_after_remove(1, 1, 2), 1);
        assert_eq!(focus_after_remove(0, 0, 2), 0);
        assert_eq!(focus_after_remove(2, 0, 2), 1);
        assert_eq!(focus_after_remove(0, 1, 2), 0);
    }
}
