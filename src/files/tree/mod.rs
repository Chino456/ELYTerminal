mod model;
mod row;

use std::{collections::HashSet, ops::Range, rc::Rc};

pub use model::{DropAt, TreeDrag, TreeNode};

use gpui::{
    App, ElementId, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, StyleRefinement, Styled, UniformListScrollHandle,
    Window, div, uniform_list,
};

use self::{
    model::rows,
    row::{OnKey, OnMove, OnToggle, Rows, draw_row},
};

struct Seeded<T> {
    seed: T,
    value: T,
}

fn use_seeded<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    seed: T,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Seeded<T>> {
    let state = window.use_keyed_state(id, cx, |_, _| Seeded {
        seed: seed.clone(),
        value: seed.clone(),
    });
    if state.read(cx).seed != seed {
        state.update(cx, |state, _| {
            state.value = seed.clone();
            state.seed = seed;
        });
    }
    state
}

#[derive(Default)]
struct Nav {
    scroll: UniformListScrollHandle,
    drop: Option<(SharedString, DropAt)>,
    asked: HashSet<SharedString>,
}

/// Nodes that open and close in a virtual list.
#[derive(IntoElement)]
pub struct Tree {
    id: ElementId,
    base: gpui::Div,
    nodes: Vec<TreeNode>,
    open: Vec<SharedString>,
    on_activate: Option<OnKey>,
    on_load: Option<OnKey>,
    on_move: Option<OnMove>,
}

impl Tree {
    pub fn new(id: impl Into<ElementId>, nodes: impl IntoIterator<Item = TreeNode>) -> Self {
        Self {
            id: id.into(),
            base: div(),
            nodes: nodes.into_iter().collect(),
            open: Vec::new(),
            on_activate: None,
            on_load: None,
            on_move: None,
        }
    }

    pub fn open(mut self, keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.open = keys.into_iter().map(Into::into).collect();
        self
    }

    pub fn on_activate(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(handler));
        self
    }

    pub fn on_load(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_load = Some(Rc::new(handler));
        self
    }

    pub fn on_move(
        mut self,
        handler: impl Fn(&SharedString, &SharedString, DropAt, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_move = Some(Rc::new(handler));
        self
    }
}

impl Styled for Tree {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Tree {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.clone();
        let nav = window.use_keyed_state((id.clone(), "nav"), cx, |_, _| Nav::default());
        if nav.read(cx).drop.is_some() && !cx.has_active_drag() {
            nav.update(cx, |nav, _| nav.drop = None);
        }
        let opened = use_seeded((id.clone(), "open"), self.open, window, cx);
        let open_set: HashSet<SharedString> = opened.read(cx).value.iter().cloned().collect();
        let nodes = Rc::new(self.nodes);
        let shown = Rc::new(rows(&nodes, &open_set));
        for row in shown
            .iter()
            .filter(|row| matches!(row.shown, model::Shown::Loading))
        {
            if let Some(parent) = row.parent
                && let Some(key) = shown[parent].key().cloned()
            {
                if nav.read(cx).asked.contains(&key) {
                    continue;
                }
                nav.update(cx, |nav, _| nav.asked.insert(key.clone()));
                if let Some(on_load) = self.on_load.clone() {
                    log::info!("tree {id:?}: asking for {key}'s children");
                    window.defer(cx, move |window, cx| on_load(&key, window, cx));
                }
            }
        }
        let count = shown.len();
        let activate: OnKey = {
            let on_activate = self.on_activate.clone();
            Rc::new(move |key, window, cx| {
                if let Some(on_activate) = &on_activate {
                    on_activate(key, window, cx);
                }
            })
        };
        let toggle: OnToggle = {
            let opened = opened.clone();
            Rc::new(move |key: &SharedString, open: bool, cx: &mut App| {
                opened.update(cx, |seeded, cx| {
                    seeded.value.retain(|known| known != key);
                    if open {
                        seeded.value.push(key.clone());
                    }
                    cx.notify();
                });
            })
        };
        let scroll = nav.read(cx).scroll.clone();
        let data = Rows {
            id: id.clone(),
            owner: nav.entity_id(),
            nodes: nodes.clone(),
            shown: shown.clone(),
            nav: nav.clone(),
            activate,
            toggle,
            on_move: self.on_move,
        };
        let list = uniform_list(
            (id.clone(), "rows"),
            count,
            move |range: Range<usize>, window, cx| {
                range
                    .map(|ix| draw_row(&data, ix, window, cx))
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&scroll)
        .size_full();
        self.base.id(self.id).role(Role::Tree).child(list)
    }
}
