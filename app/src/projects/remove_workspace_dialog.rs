use pathfinder_geometry::vector::vec2f;
use warp_core::ui::theme::Fill;
use warpui::elements::{
    Align, ChildAnchor, ChildView, Container, OffsetPositioning, ParentAnchor, ParentOffsetBounds,
    Stack,
};
use warpui::keymap::{FixedBinding, Keystroke};
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::ProjectId;
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
            RemoveWorkspaceAction::Cancel,
            id!(RemoveWorkspaceDialog::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            RemoveWorkspaceAction::Confirm,
            id!(RemoveWorkspaceDialog::ui_name()),
        ),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveTarget {
    Project(ProjectId),
    Home,
}

pub enum RemoveWorkspaceEvent {
    Confirm { target: RemoveTarget },
    Cancel,
}

#[derive(Debug)]
pub enum RemoveWorkspaceAction {
    Confirm,
    Cancel,
}

pub struct RemoveWorkspaceDialog {
    cancel_button: ViewHandle<ActionButton>,
    confirm_button: ViewHandle<ActionButton>,
    target: Option<RemoveTarget>,
    display_name: String,
    note: Option<String>,
}

impl RemoveWorkspaceDialog {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(RemoveWorkspaceAction::Cancel);
            })
        });

        let enter_keystroke = Keystroke::parse("enter").expect("Valid keystroke");
        let confirm_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Remove", DangerPrimaryTheme)
                .with_keybinding(KeystrokeSource::Fixed(enter_keystroke), ctx)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(RemoveWorkspaceAction::Confirm);
                })
        });

        Self {
            cancel_button,
            confirm_button,
            target: None,
            display_name: String::new(),
            note: None,
        }
    }

    pub fn set_target(
        &mut self,
        target: RemoveTarget,
        display_name: String,
        note: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.target = Some(target);
        self.display_name = display_name;
        self.note = note;
        let confirm_label = match target {
            RemoveTarget::Project(_) => "Remove",
            RemoveTarget::Home => "Close",
        };
        self.confirm_button
            .update(ctx, |button, ctx| button.set_label(confirm_label, ctx));
    }
}

impl Entity for RemoveWorkspaceDialog {
    type Event = RemoveWorkspaceEvent;
}

impl View for RemoveWorkspaceDialog {
    fn ui_name() -> &'static str {
        "RemoveWorkspaceDialog"
    }

    fn on_focus(&mut self, _focus_ctx: &warpui::FocusContext, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);

        let cancel_button = Container::new(ChildView::new(&self.cancel_button).finish())
            .with_margin_right(12.)
            .finish();

        let is_home = matches!(self.target, Some(RemoveTarget::Home));
        let title = if is_home {
            "Close Home?".to_owned()
        } else {
            format!("Remove '{}' from Spirit?", self.display_name)
        };
        let mut body = if is_home {
            "Home comes back the next time you switch to it.".to_owned()
        } else {
            "The folder on disk is left untouched; only Spirit forgets this Workspace.".to_owned()
        };
        if let Some(note) = &self.note {
            body.push(' ');
            body.push_str(note);
        }

        let dialog = Dialog::new(
            title,
            Some(body),
            UiComponentStyles {
                width: Some(DIALOG_WIDTH),
                ..dialog_styles(appearance)
            },
        )
        .with_bottom_row_child(cancel_button)
        .with_bottom_row_child(ChildView::new(&self.confirm_button).finish())
        .build()
        .finish();

        let mut stack = Stack::new();
        stack.add_positioned_child(
            dialog,
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

impl TypedActionView for RemoveWorkspaceDialog {
    type Action = RemoveWorkspaceAction;

    fn handle_action(&mut self, action: &RemoveWorkspaceAction, ctx: &mut ViewContext<Self>) {
        match action {
            RemoveWorkspaceAction::Confirm => {
                let Some(target) = self.target else {
                    return;
                };
                ctx.emit(RemoveWorkspaceEvent::Confirm { target });
            }
            RemoveWorkspaceAction::Cancel => ctx.emit(RemoveWorkspaceEvent::Cancel),
        }
    }
}
