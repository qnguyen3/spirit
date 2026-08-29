use pathfinder_geometry::vector::vec2f;
use warp_core::ui::theme::Fill;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, ChildAnchor, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    Element, Empty, Flex, Hoverable, MouseStateHandle, OffsetPositioning, ParentAnchor,
    ParentElement, ParentOffsetBounds, Radius, Stack, Text,
};
use warpui::keymap::{FixedBinding, Keystroke};
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::WorktreeId;
use crate::appearance::Appearance;
use crate::ui_components::dialog::{Dialog, dialog_styles};
use crate::view_components::action_button::{
    ActionButton, DangerPrimaryTheme, KeystrokeSource, NakedTheme,
};

const DIALOG_WIDTH: f32 = 460.;

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_fixed_bindings([
        FixedBinding::new(
            "escape",
            DeleteWorktreeAction::Cancel,
            id!(DeleteWorktreeDialog::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            DeleteWorktreeAction::Confirm,
            id!(DeleteWorktreeDialog::ui_name()),
        ),
    ]);
}

pub enum DeleteWorktreeEvent {
    Confirm {
        worktree_id: WorktreeId,
        force: bool,
    },
    Cancel,
}

#[derive(Debug)]
pub enum DeleteWorktreeAction {
    Confirm,
    Cancel,
    ToggleForce,
}

pub struct DeleteWorktreeDialog {
    cancel_button: ViewHandle<ActionButton>,
    confirm_button: ViewHandle<ActionButton>,
    worktree_id: Option<WorktreeId>,
    name: String,
    branch: String,
    dirty: Option<bool>,
    force: bool,
    force_mouse_state: MouseStateHandle,
}

impl DeleteWorktreeDialog {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(DeleteWorktreeAction::Cancel);
            })
        });

        let enter_keystroke = Keystroke::parse("enter").expect("Valid keystroke");
        let confirm_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Delete", DangerPrimaryTheme)
                .with_keybinding(KeystrokeSource::Fixed(enter_keystroke), ctx)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(DeleteWorktreeAction::Confirm);
                })
        });

        Self {
            cancel_button,
            confirm_button,
            worktree_id: None,
            name: String::new(),
            branch: String::new(),
            dirty: None,
            force: false,
            force_mouse_state: MouseStateHandle::default(),
        }
    }

    pub fn set_target(&mut self, worktree_id: WorktreeId, name: String, branch: String) {
        self.worktree_id = Some(worktree_id);
        self.name = name;
        self.branch = branch;
        self.dirty = None;
        self.force = false;
    }

    pub fn set_dirty(&mut self, dirty: bool, ctx: &mut ViewContext<Self>) {
        self.dirty = Some(dirty);
        ctx.notify();
    }

    fn can_confirm(&self) -> bool {
        match self.dirty {
            None => false,
            Some(true) => self.force,
            Some(false) => true,
        }
    }

    fn render_force_row(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let checked = self.force;
        let font_family = appearance.ui_font_family();
        let font_size = appearance.ui_font_size();
        Hoverable::new(self.force_mouse_state.clone(), move |state| {
            let box_fill = if checked {
                theme.accent()
            } else if state.is_hovered() {
                internal_colors::fg_overlay_2(theme)
            } else {
                internal_colors::fg_overlay_1(theme)
            };
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    ConstrainedBox::new(
                        Container::new(Empty::new().finish())
                            .with_background(box_fill)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
                            .finish(),
                    )
                    .with_width(14.)
                    .with_height(14.)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(
                        "Discard the uncommitted changes".to_owned(),
                        font_family,
                        font_size,
                    )
                    .with_color(theme.foreground().into())
                    .finish(),
                )
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(DeleteWorktreeAction::ToggleForce);
        })
        .finish()
    }
}

impl Entity for DeleteWorktreeDialog {
    type Event = DeleteWorktreeEvent;
}

impl View for DeleteWorktreeDialog {
    fn ui_name() -> &'static str {
        "DeleteWorktreeDialog"
    }

    fn on_focus(&mut self, _focus_ctx: &warpui::FocusContext, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);

        let title = format!("Delete worktree '{}'?", self.name);
        let body = match self.dirty {
            None => "Checking for uncommitted changes\u{2026}".to_owned(),
            Some(false) => format!(
                "This removes its folder and deletes branch {} if it is fully merged.",
                self.branch
            ),
            Some(true) => format!(
                "'{}' has uncommitted changes. Deleting it discards them and removes branch {} if it is fully merged.",
                self.name, self.branch
            ),
        };

        let mut dialog = Dialog::new(
            title,
            Some(body),
            UiComponentStyles {
                width: Some(DIALOG_WIDTH),
                ..dialog_styles(appearance)
            },
        );

        if self.dirty == Some(true) {
            dialog = dialog.with_child(self.render_force_row(appearance));
        }

        let cancel_button = Container::new(ChildView::new(&self.cancel_button).finish())
            .with_margin_right(12.)
            .finish();

        let mut bottom_row = vec![cancel_button];
        if self.can_confirm() {
            bottom_row.push(ChildView::new(&self.confirm_button).finish());
        }
        for child in bottom_row {
            dialog = dialog.with_bottom_row_child(child);
        }

        let mut stack = Stack::new();
        stack.add_positioned_child(
            dialog.build().finish(),
            OffsetPositioning::offset_from_parent(
                vec2f(0., 0.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );

        Container::new(Align::new(stack.finish()).finish())
            .with_background_color(Fill::blur().into())
            .with_corner_radius(app.windows().window_corner_radius())
            .finish()
    }
}

impl TypedActionView for DeleteWorktreeDialog {
    type Action = DeleteWorktreeAction;

    fn handle_action(&mut self, action: &DeleteWorktreeAction, ctx: &mut ViewContext<Self>) {
        match action {
            DeleteWorktreeAction::Confirm => {
                if !self.can_confirm() {
                    return;
                }
                let Some(worktree_id) = self.worktree_id else {
                    return;
                };
                ctx.emit(DeleteWorktreeEvent::Confirm {
                    worktree_id,
                    force: self.force,
                });
            }
            DeleteWorktreeAction::Cancel => ctx.emit(DeleteWorktreeEvent::Cancel),
            DeleteWorktreeAction::ToggleForce => {
                self.force = !self.force;
                ctx.notify();
            }
        }
    }
}

#[cfg(test)]
#[path = "delete_worktree_dialog_tests.rs"]
mod tests;
