use std::collections::HashSet;

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, ChildAnchor, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    Element, Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning,
    Padding, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, Shrinkable, Stack, Text,
};
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::git_ops::sanitize_worktree_name;
use crate::agent_launcher::catalog::{self, AgentDefinition};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions};
use crate::menu::{Event as MenuEvent, Menu, MenuItem, MenuItemFields};

const SECTION_GAP: f32 = 14.;
const CONTENT_PADDING: f32 = 24.;
const TITLE_FONT_SIZE: f32 = 16.;
const LABEL_FONT_SIZE: f32 = 12.;
const BUTTON_HEIGHT: f32 = 32.;
const BUTTON_RADIUS: Radius = Radius::Pixels(4.);
const ERROR_TEXT_COLOR: u32 = 0xBC362AFF;

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;
    app.register_fixed_bindings(vec![FixedBinding::new(
        "escape",
        CreateWorktreeModalAction::Escape,
        id!("CreateWorktreeModal"),
    )]);
}

#[derive(Debug, Clone)]
pub enum CreateWorktreeModalEvent {
    Close,
    Submit {
        name: String,
        agent_catalog_index: Option<usize>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum CreateWorktreeModalAction {
    Regenerate,
    ShowAgentMenu,
    SelectAgent(Option<usize>),
    Submit,
    Cancel,
    Escape,
}

pub struct CreateWorktreeModal {
    name_editor: ViewHandle<EditorView>,
    primary_branch: String,
    agent_menu: ViewHandle<Menu<CreateWorktreeModalAction>>,
    show_agent_menu: bool,
    selected_agent: Option<usize>,
    installed_agents: Vec<bool>,
    shell_path_env: Option<String>,
    existing_branches: HashSet<String>,
    generated_name: Option<String>,
    in_flight: bool,
    error: Option<String>,
    regenerate_mouse_state: MouseStateHandle,
    agent_mouse_state: MouseStateHandle,
    submit_mouse_state: MouseStateHandle,
    cancel_mouse_state: MouseStateHandle,
}

impl Entity for CreateWorktreeModal {
    type Event = CreateWorktreeModalEvent;
}

impl TypedActionView for CreateWorktreeModal {
    type Action = CreateWorktreeModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            CreateWorktreeModalAction::Regenerate => self.regenerate_name(ctx),
            CreateWorktreeModalAction::ShowAgentMenu => self.show_agent_menu(ctx),
            CreateWorktreeModalAction::SelectAgent(index) => {
                self.selected_agent = *index;
                self.show_agent_menu = false;
                ctx.notify();
            }
            CreateWorktreeModalAction::Submit => self.try_submit(ctx),
            CreateWorktreeModalAction::Cancel | CreateWorktreeModalAction::Escape => {
                ctx.emit(CreateWorktreeModalEvent::Close)
            }
        }
    }
}

impl CreateWorktreeModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let name_editor = ctx.add_typed_action_view(|ctx| {
            let mut editor = EditorView::single_line(SingleLineEditorOptions::default(), ctx);
            editor.set_placeholder_text("worktree name", ctx);
            editor
        });
        ctx.subscribe_to_view(&name_editor, |me, _, event, ctx| match event {
            EditorEvent::Enter => me.try_submit(ctx),
            EditorEvent::Escape => ctx.emit(CreateWorktreeModalEvent::Close),
            EditorEvent::Edited(_) => {
                me.error = None;
                ctx.notify();
            }
            _ => {}
        });

        let agent_menu = ctx.add_typed_action_view(|_| {
            Menu::new()
                .with_drop_shadow()
                .prevent_interaction_with_other_elements()
        });
        ctx.subscribe_to_view(&agent_menu, |me, _, event, ctx| {
            if let MenuEvent::Close { .. } = event {
                me.show_agent_menu = false;
                ctx.notify();
            }
        });

        let installed_agents = catalog::agent_catalog()
            .iter()
            .map(|definition| catalog::is_installed(definition, None))
            .collect();

        Self {
            name_editor,
            primary_branch: String::new(),
            agent_menu,
            show_agent_menu: false,
            selected_agent: None,
            installed_agents,
            shell_path_env: None,
            existing_branches: HashSet::new(),
            generated_name: None,
            in_flight: false,
            error: None,
            regenerate_mouse_state: MouseStateHandle::default(),
            agent_mouse_state: MouseStateHandle::default(),
            submit_mouse_state: MouseStateHandle::default(),
            cancel_mouse_state: MouseStateHandle::default(),
        }
    }

    pub fn on_open(
        &mut self,
        primary_branch: String,
        existing_branches: HashSet<String>,
        agent_catalog_index: Option<usize>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.primary_branch = primary_branch;
        self.existing_branches = existing_branches;
        self.selected_agent = agent_catalog_index;
        self.show_agent_menu = false;
        self.in_flight = false;
        self.error = None;
        self.regenerate_name(ctx);
        self.request_shell_path(ctx);
        ctx.focus(&self.name_editor);
        ctx.notify();
    }

    pub fn extend_existing_branches(
        &mut self,
        branches: HashSet<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.existing_branches.extend(branches);
        let typed = self.name_editor.as_ref(ctx).buffer_text(ctx);
        let untouched = self.generated_name.as_deref() == Some(typed.as_str());
        if untouched && self.existing_branches.contains(&typed) {
            self.regenerate_name(ctx);
        }
        ctx.notify();
    }

    pub fn set_error(&mut self, error: String, ctx: &mut ViewContext<Self>) {
        self.error = Some(error);
        self.in_flight = false;
        ctx.notify();
    }

    fn request_shell_path(&mut self, ctx: &mut ViewContext<Self>) {
        if self.shell_path_env.is_some() {
            return;
        }
        #[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
        {
            use crate::terminal::local_shell::LocalShellState;

            if !ctx.has_singleton_model::<LocalShellState>() {
                return;
            }
            let path_future = LocalShellState::handle(ctx).update(ctx, |shell_state, ctx| {
                shell_state.get_interactive_path_env_var(ctx)
            });
            ctx.spawn(path_future, |me: &mut Self, path_env, ctx| {
                if path_env.is_some() {
                    me.shell_path_env = path_env;
                    me.refresh_install_state(ctx);
                }
            });
        }
        #[cfg(any(target_family = "wasm", not(feature = "local_tty")))]
        {
            let _ = ctx;
        }
    }

    fn refresh_install_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.installed_agents = catalog::agent_catalog()
            .iter()
            .map(|definition| catalog::is_installed(definition, self.shell_path_env.as_deref()))
            .collect();
        ctx.notify();
    }

    fn regenerate_name(&mut self, ctx: &mut ViewContext<Self>) {
        let taken: HashSet<&str> = self.existing_branches.iter().map(String::as_str).collect();
        let name = warp_util::worktree_names::generate_worktree_branch_name(&taken);
        self.name_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(&name, ctx);
        });
        self.generated_name = Some(name);
        ctx.notify();
    }

    fn show_agent_menu(&mut self, ctx: &mut ViewContext<Self>) {
        let mut items = vec![
            MenuItemFields::new("None (terminal)")
                .with_on_select_action(CreateWorktreeModalAction::SelectAgent(None))
                .into_item(),
            MenuItem::Separator,
        ];
        for (index, definition) in catalog::agent_catalog().iter().enumerate() {
            let installed = self.installed_agents.get(index).copied().unwrap_or(false);
            let label = if installed {
                definition.display_name.to_owned()
            } else {
                format!("{} (not installed)", definition.display_name)
            };
            items.push(
                MenuItemFields::new(label)
                    .with_disabled(!installed)
                    .with_on_select_action(CreateWorktreeModalAction::SelectAgent(Some(index)))
                    .into_item(),
            );
        }
        self.agent_menu
            .update(ctx, |menu, ctx| menu.set_items(items, ctx));
        self.show_agent_menu = true;
        ctx.focus(&self.agent_menu);
        ctx.notify();
    }

    fn selected_agent_definition(&self) -> Option<&'static AgentDefinition> {
        let index = self.selected_agent?;
        catalog::agent_catalog().get(index)
    }

    fn try_submit(&mut self, ctx: &mut ViewContext<Self>) {
        if self.in_flight {
            return;
        }
        let raw = self.name_editor.as_ref(ctx).buffer_text(ctx);
        let name = sanitize_worktree_name(&raw);
        if raw.trim().is_empty() {
            self.error = Some("Enter a name for the Worktree".to_owned());
            ctx.notify();
            return;
        }
        self.in_flight = true;
        ctx.notify();
        ctx.emit(CreateWorktreeModalEvent::Submit {
            name,
            agent_catalog_index: self.selected_agent,
        });
    }

    fn render_agent_control(&self, label: String, appearance: &Appearance) -> Box<dyn Element> {
        let button = Self::render_button(
            label,
            self.agent_mouse_state.clone(),
            false,
            !self.in_flight,
            CreateWorktreeModalAction::ShowAgentMenu,
            appearance,
        );
        if !self.show_agent_menu {
            return button;
        }

        let mut stack = Stack::new().with_child(button);
        stack.add_positioned_overlay_child(
            ChildView::new(&self.agent_menu).finish(),
            OffsetPositioning::offset_from_parent(
                vec2f(0., 4.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::BottomLeft,
                ChildAnchor::TopLeft,
            ),
        );
        stack.finish()
    }

    fn name_input_styles(appearance: &Appearance) -> UiComponentStyles {
        let theme = appearance.theme();
        UiComponentStyles::default()
            .set_background(theme.background().into())
            .set_border_radius(CornerRadius::with_all(BUTTON_RADIUS))
            .set_border_width(1.)
            .set_border_color(theme.foreground().with_opacity(20).into())
            .set_padding(Coords::uniform(6.).left(8.).right(8.))
    }

    fn render_button(
        label: String,
        mouse_state: MouseStateHandle,
        emphasized: bool,
        enabled: bool,
        action: CreateWorktreeModalAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let font_family = appearance.ui_font_family();
        let font_size = appearance.ui_font_size();
        let text_color = if enabled {
            theme.foreground()
        } else {
            theme.disabled_text_color(theme.background())
        };
        let button = Hoverable::new(mouse_state, move |state| {
            let background = if !enabled {
                theme.background()
            } else if emphasized {
                theme.accent()
            } else if state.is_hovered() {
                internal_colors::fg_overlay_2(theme)
            } else {
                internal_colors::fg_overlay_1(theme)
            };
            ConstrainedBox::new(
                Container::new(
                    Align::new(
                        Text::new_inline(label.clone(), font_family, font_size)
                            .with_color(text_color.into())
                            .finish(),
                    )
                    .finish(),
                )
                .with_background(background)
                .with_corner_radius(CornerRadius::with_all(BUTTON_RADIUS))
                .with_horizontal_padding(12.)
                .finish(),
            )
            .with_height(BUTTON_HEIGHT)
            .finish()
        })
        .with_cursor(if enabled {
            Cursor::PointingHand
        } else {
            Cursor::Arrow
        });

        if enabled {
            button
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(action);
                })
                .finish()
        } else {
            button.finish()
        }
    }
}

impl View for CreateWorktreeModal {
    fn ui_name() -> &'static str {
        "CreateWorktreeModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let muted = theme.sub_text_color(theme.background());

        let typed = self.name_editor.as_ref(app).buffer_text(app);
        let branch = sanitize_worktree_name(&typed);
        let base = if self.primary_branch.is_empty() {
            "the primary branch".to_owned()
        } else {
            self.primary_branch.clone()
        };

        let agent_label = match self.selected_agent_definition() {
            Some(definition) => format!("Agent: {}", definition.display_name),
            None => "Agent: None (terminal)".to_owned(),
        };

        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(SECTION_GAP)
            .with_children([
                Text::new_inline(
                    "New Worktree".to_owned(),
                    appearance.ui_font_family(),
                    TITLE_FONT_SIZE,
                )
                .with_color(theme.foreground().into())
                .finish(),
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(
                        Shrinkable::new(
                            1.,
                            TextInput::new(
                                self.name_editor.clone(),
                                Self::name_input_styles(appearance),
                            )
                            .build()
                            .finish(),
                        )
                        .finish(),
                    )
                    .with_child(Self::render_button(
                        "\u{1F3B2}".to_owned(),
                        self.regenerate_mouse_state.clone(),
                        false,
                        !self.in_flight,
                        CreateWorktreeModalAction::Regenerate,
                        appearance,
                    ))
                    .finish(),
                Text::new_inline(
                    format!("Branch {branch} from {base}"),
                    appearance.monospace_font_family(),
                    LABEL_FONT_SIZE,
                )
                .with_color(muted.into())
                .finish(),
                self.render_agent_control(agent_label, appearance),
            ]);

        if let Some(error) = &self.error {
            body.add_child(
                Text::new_inline(error.clone(), appearance.ui_font_family(), LABEL_FONT_SIZE)
                    .with_color(ColorU::from_u32(ERROR_TEXT_COLOR))
                    .finish(),
            );
        }

        body.add_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_children([
                    Self::render_button(
                        "Cancel".to_owned(),
                        self.cancel_mouse_state.clone(),
                        false,
                        true,
                        CreateWorktreeModalAction::Cancel,
                        appearance,
                    ),
                    Self::render_button(
                        if self.in_flight {
                            "Creating\u{2026}".to_owned()
                        } else {
                            "Create".to_owned()
                        },
                        self.submit_mouse_state.clone(),
                        true,
                        !self.in_flight,
                        CreateWorktreeModalAction::Submit,
                        appearance,
                    ),
                ])
                .finish(),
        );

        Container::new(body.finish())
            .with_padding(Padding::uniform(CONTENT_PADDING))
            .finish()
    }
}

#[cfg(test)]
#[path = "create_worktree_modal_tests.rs"]
mod tests;
