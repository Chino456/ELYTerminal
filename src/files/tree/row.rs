use std::rc::Rc;

use ely_gpui_component::{
    primitives::{Disclosure, DragGhost, Icon},
    theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, DragMoveEvent, ElementId, Entity, EntityId, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Role, SharedString, Styled, Window, div, prelude::*, relative,
};

use super::{
    Nav,
    model::{DropAt, Row, Shown, TreeDrag, TreeNode, can_drop, place_at},
};

pub(super) type OnKey = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;
pub(super) type OnMove = Rc<dyn Fn(&SharedString, &SharedString, DropAt, &mut Window, &mut App)>;
pub(super) type OnToggle = Rc<dyn Fn(&SharedString, bool, &mut App)>;

pub(super) struct Rows {
    pub id: ElementId,
    pub owner: EntityId,
    pub nodes: Rc<Vec<TreeNode>>,
    pub shown: Rc<Vec<Row>>,
    pub nav: Entity<Nav>,
    pub activate: OnKey,
    pub toggle: OnToggle,
    pub on_move: Option<OnMove>,
}

fn tone_color(
    tone: ely_gpui_component::data_display::Tone,
    colors: &ely_gpui_component::theme::Palette,
) -> gpui::Hsla {
    match tone {
        ely_gpui_component::data_display::Tone::Neutral => colors.fg_muted,
        ely_gpui_component::data_display::Tone::Accent => colors.accent,
        ely_gpui_component::data_display::Tone::Info => colors.info,
        ely_gpui_component::data_display::Tone::Success => colors.success,
        ely_gpui_component::data_display::Tone::Warning => colors.warning,
        ely_gpui_component::data_display::Tone::Danger => colors.danger,
    }
}

fn guides(depth: usize, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .absolute()
        .inset_0()
        .px_2()
        .flex()
        .children((0..depth).map(|_| {
            div()
                .flex_none()
                .w(theme.tree_indent())
                .h_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .h_full()
                        .border_l_1()
                        .border_color(theme.colors.border),
                )
        }))
}

pub(super) fn draw_row(rows: &Rows, ix: usize, _: &mut Window, cx: &mut App) -> AnyElement {
    let item = &rows.shown[ix];
    let (key, path, label, icon, opens, open) = match &item.shown {
        Shown::Node {
            key,
            path,
            label,
            icon,
            opens,
            open,
            ..
        } => (
            key.clone(),
            path.clone(),
            label.clone(),
            *icon,
            *opens,
            *open,
        ),
        Shown::Loading => {
            let theme = cx.theme();
            return div()
                .relative()
                .w_full()
                .h(theme.control_height(ControlSize::Md))
                .flex()
                .items_center()
                .px_2()
                .child(guides(item.depth, cx))
                .child(
                    div()
                        .flex_none()
                        .w(theme.tree_indent() * (item.depth + 1) as f32),
                )
                .child(
                    ely_gpui_component::motion::Spinner::new((
                        rows.id.clone(),
                        format!("spinner-{ix}"),
                    ))
                    .size(IconSize::Xs),
                )
                .into_any_element();
        }
    };
    let theme = cx.theme();
    let colors = &theme.colors;
    let indent = theme.tree_indent();
    let nav = rows.nav.read(cx);
    let landing = nav
        .drop
        .as_ref()
        .filter(|(target, _)| *target == key)
        .map(|(_, at)| *at);
    let (note, ink) = match &item.shown {
        Shown::Node { note, tone, .. } => (note.clone(), tone.map(|tone| tone_color(tone, colors))),
        Shown::Loading => (None, None),
    };
    let (toggle, toggle_click, activate) = (
        rows.toggle.clone(),
        rows.toggle.clone(),
        rows.activate.clone(),
    );
    let (owner, nodes, nav_entity, on_move) = (
        rows.owner,
        rows.nodes.clone(),
        rows.nav.clone(),
        rows.on_move.clone(),
    );
    let (dropped_on, dragged_over, shut, act_key) =
        (key.clone(), key.clone(), key.clone(), key.clone());
    let chevron = div()
        .flex_none()
        .w(indent)
        .ml(indent * item.depth as f32)
        .flex()
        .justify_center()
        .when(opens, |slot| {
            slot.child(Disclosure::new(
                (rows.id.clone(), format!("chevron-{key}")),
                open,
            ))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                toggle(&shut, !open, cx);
            })
        });
    let line = |at: DropAt| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .when(at == DropAt::Before, |line| line.top_0())
            .when(at == DropAt::After, |line| line.bottom_0())
            .border_t_2()
            .border_color(colors.accent)
    };
    div()
        .id((rows.id.clone(), format!("node-{key}")))
        .role(Role::TreeItem)
        .aria_label(label.clone())
        .aria_level(item.depth + 1)
        .when(opens, |row| row.aria_expanded(open))
        .relative()
        .w_full()
        .h(theme.control_height(ControlSize::Md))
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .rounded(theme.radius(Radius::Sm))
        .text_size(theme.text_size(TextSize::Sm))
        .text_color(colors.fg)
        .hover(|style| style.bg(colors.hover))
        .when(landing == Some(DropAt::Inside), |row| {
            row.bg(colors.selection)
        })
        .child(guides(item.depth, cx))
        .child(chevron)
        .children(icon.map(|icon| Icon::new(icon).size(IconSize::Sm).color(colors.fg_muted)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when_some(ink, |text, ink| text.text_color(ink))
                .child(Ellipsis::new(label.clone())),
        )
        .children(note.map(|note| {
            div()
                .flex_none()
                .max_w(relative(0.5))
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(ink.unwrap_or(colors.fg_subtle))
                .child(Ellipsis::new(note))
        }))
        .children(landing.filter(|at| *at != DropAt::Inside).map(line))
        .on_click(move |event, window, cx| {
            if opens {
                toggle_click(&act_key, !open, cx);
            } else if event.click_count() == 2 {
                activate(&act_key, window, cx);
            }
        })
        .on_drag(
            TreeDrag {
                owner,
                path,
                key: key.clone(),
                label: label.clone(),
            },
            move |drag, _, _, cx| DragGhost::new(drag.label.clone(), icon, cx),
        )
        .on_drag_move(move |event: &DragMoveEvent<TreeDrag>, _, cx| {
            let drag = event.drag(cx);
            if drag.owner != owner || !event.bounds.contains(&event.event.position) {
                return;
            }
            let share = (event.event.position.y - event.bounds.top()) / event.bounds.size.height;
            let fits = drag.key != dragged_over && can_drop(&nodes, &drag.key, &dragged_over);
            let next = fits.then(|| (dragged_over.clone(), place_at(share, opens)));
            nav_entity.update(cx, |nav, cx| {
                if nav.drop != next {
                    nav.drop = next;
                    cx.notify();
                }
            });
        })
        .on_drop({
            let nav = rows.nav.clone();
            move |drag: &TreeDrag, window, cx| {
                if drag.owner != owner {
                    return;
                }
                let landed = nav.update(cx, |nav, cx| {
                    cx.notify();
                    nav.drop.take()
                });
                if let (Some((target, at)), Some(on_move)) = (landed, &on_move)
                    && target == dropped_on
                {
                    log::info!("tree: {} dropped {at:?} {target}", drag.key);
                    on_move(&drag.key, &target, at, window, cx);
                }
            }
        })
        .into_any_element()
}
