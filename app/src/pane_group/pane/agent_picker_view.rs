use warp_core::ui::appearance::Appearance;
use warp_core::ui::color::blend::Blend as _;
use warp_core::ui::theme::Fill;
use warpui::elements::{
    Align, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult,
    EventHandler, Flex, Hoverable, MouseStateHandle, ParentElement as _, Radius,
};
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, ModelHandle, SingletonEntity as _, TypedActionView, View,
    ViewContext,
};

use crate::agent_launcher::catalog::{self, AgentDefinition};
use crate::pane_group::focus_state::PaneFocusHandle;
use crate::pane_group::pane::view;
use crate::pane_group::{BackingView, PaneConfiguration, PaneEvent};
#[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
use crate::terminal::local_shell::LocalShellState;
use crate::ui_components::icons::Icon;
use crate::workspace::WorkspaceAction;

pub const AGENT_PICKER_PANE_TITLE: &str = "New Agent";

const CONTENT_MAX_WIDTH: f32 = 560.;
const TITLE_FONT_SIZE: f32 = 20.;
const SUBTITLE_FONT_SIZE: f32 = 14.;
const NAME_FONT_SIZE: f32 = 14.;
const DETAIL_FONT_SIZE: f32 = 12.;
const ICON_GLYPH_SIZE: f32 = 14.;
const ICON_CIRCLE_PADDING: f32 = 6.;
const ROW_CORNER_RADIUS: f32 = 6.;
const ROW_HORIZONTAL_PADDING: f32 = 10.;
const ROW_VERTICAL_PADDING: f32 = 8.;
const ROW_DETAIL_INSET: f32 = ICON_GLYPH_SIZE + 2. * ICON_CIRCLE_PADDING + ROW_ICON_MARGIN_RIGHT;
const ROW_ICON_MARGIN_RIGHT: f32 = 10.;
const CHEVRON_SIZE: f32 = 14.;
const CHEVRON_MARGIN_RIGHT: f32 = 6.;
const NOT_INSTALLED_SECTION_MARGIN_TOP: f32 = 8.;

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_fixed_bindings([
        FixedBinding::new("up", AgentPickerAction::Up, id!(AgentPickerView::ui_name())),
        FixedBinding::new(
            "down",
            AgentPickerAction::Down,
            id!(AgentPickerView::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            AgentPickerAction::Confirm,
            id!(AgentPickerView::ui_name()),
        ),
        FixedBinding::new(
            "escape",
            AgentPickerAction::Close,
            id!(AgentPickerView::ui_name()),
        ),
    ]);
}

#[derive(Debug)]
pub enum AgentPickerAction {
    Up,
    Down,
    Confirm,
    Close,
    Select(usize),
    ToggleNotInstalled,
}

#[derive(Debug)]
pub enum AgentPickerViewEvent {
    Pane(PaneEvent),
}

struct AgentPickerRow {
    is_installed: bool,
    mouse_state: MouseStateHandle,
    install_link_mouse_state: MouseStateHandle,
}

pub struct AgentPickerView {
    pane_configuration: ModelHandle<PaneConfiguration>,
    focus_handle: Option<PaneFocusHandle>,
    rows: Vec<AgentPickerRow>,
    selected_index: Option<usize>,
    shell_path_env: Option<String>,
    shell_path_requested: bool,
    not_installed_expanded: bool,
    not_installed_header_mouse_state: MouseStateHandle,
}

impl AgentPickerView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let pane_configuration =
            ctx.add_model(|_ctx| PaneConfiguration::new(AGENT_PICKER_PANE_TITLE));
        let rows: Vec<AgentPickerRow> = catalog::agent_catalog()
            .iter()
            .map(|def| AgentPickerRow {
                is_installed: catalog::is_installed(def, None),
                mouse_state: MouseStateHandle::default(),
                install_link_mouse_state: MouseStateHandle::default(),
            })
            .collect();
        let selected_index = rows.iter().position(|row| row.is_installed);
        Self {
            pane_configuration,
            focus_handle: None,
            rows,
            selected_index,
            shell_path_env: None,
            shell_path_requested: false,
            not_installed_expanded: selected_index.is_none(),
            not_installed_header_mouse_state: MouseStateHandle::default(),
        }
    }

    pub fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn refresh_install_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.request_shell_path(ctx);
        self.apply_install_state(ctx);
    }

    fn request_shell_path(&mut self, ctx: &mut ViewContext<Self>) {
        if self.shell_path_requested {
            return;
        }
        #[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
        {
            if !ctx.has_singleton_model::<LocalShellState>() {
                return;
            }
            self.shell_path_requested = true;
            let path_future = LocalShellState::handle(ctx).update(ctx, |shell_state, ctx| {
                shell_state.get_interactive_path_env_var(ctx)
            });
            ctx.spawn(path_future, |me, path_env, ctx| {
                if path_env.is_some() {
                    me.shell_path_env = path_env;
                    me.apply_install_state(ctx);
                }
            });
        }
        #[cfg(any(target_family = "wasm", not(feature = "local_tty")))]
        {
            self.shell_path_requested = true;
            let _ = ctx;
        }
    }

    fn apply_install_state(&mut self, ctx: &mut ViewContext<Self>) {
        let path_env = self.shell_path_env.clone();
        let mut changed = false;
        for (row, def) in self.rows.iter_mut().zip(catalog::agent_catalog()) {
            let is_installed = catalog::is_installed(def, path_env.as_deref());
            if row.is_installed != is_installed {
                row.is_installed = is_installed;
                changed = true;
            }
        }
        if changed {
            if self
                .selected_index
                .is_none_or(|index| !self.rows[index].is_installed)
            {
                self.selected_index = self.rows.iter().position(|row| row.is_installed);
            }
            self.not_installed_expanded = !self.rows.iter().any(|row| row.is_installed);
            ctx.notify();
        }
    }

    #[cfg(test)]
    fn set_install_state_for_tests(&mut self, installed: &[bool]) {
        for (row, is_installed) in self.rows.iter_mut().zip(installed) {
            row.is_installed = *is_installed;
        }
        self.selected_index = self.rows.iter().position(|row| row.is_installed);
    }

    #[cfg(test)]
    fn selected_index_for_tests(&self) -> Option<usize> {
        self.selected_index
    }

    #[cfg(test)]
    fn apply_shell_path_for_tests(&mut self, path_env: String, ctx: &mut ViewContext<Self>) {
        self.shell_path_env = Some(path_env);
        self.apply_install_state(ctx);
    }

    #[cfg(test)]
    fn install_state_for_tests(&self) -> Vec<bool> {
        self.rows.iter().map(|row| row.is_installed).collect()
    }

    #[cfg(test)]
    fn not_installed_expanded_for_tests(&self) -> bool {
        self.not_installed_expanded
    }

    fn installed_indices(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.is_installed)
            .map(|(index, _)| index)
            .collect()
    }

    fn select_next(&mut self, ctx: &mut ViewContext<Self>) {
        self.move_selection(1, ctx);
    }

    fn select_previous(&mut self, ctx: &mut ViewContext<Self>) {
        self.move_selection(-1, ctx);
    }

    fn move_selection(&mut self, delta: isize, ctx: &mut ViewContext<Self>) {
        let installed = self.installed_indices();
        if installed.is_empty() {
            return;
        }
        let len = installed.len() as isize;
        let new_index = match self
            .selected_index
            .and_then(|current| installed.iter().position(|&index| index == current))
        {
            Some(position) => installed[(position as isize + delta).rem_euclid(len) as usize],
            None => {
                if delta > 0 {
                    installed[0]
                } else {
                    installed[installed.len() - 1]
                }
            }
        };
        self.selected_index = Some(new_index);
        ctx.notify();
    }

    fn launch(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if !self.rows.get(index).is_some_and(|row| row.is_installed) {
            return;
        }
        self.selected_index = Some(index);
        ctx.dispatch_typed_action(&WorkspaceAction::LaunchAgentFromPicker {
            catalog_index: index,
        });
    }

    fn render_not_installed_header(&self, count: usize, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let muted_color = theme.disabled_text_color(theme.background()).into_solid();
        let chevron = if self.not_installed_expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };

        Hoverable::new(
            self.not_installed_header_mouse_state.clone(),
            move |state| {
                let label = appearance
                    .ui_builder()
                    .paragraph(format!("Not installed ({count})"))
                    .with_style(UiComponentStyles {
                        font_size: Some(DETAIL_FONT_SIZE),
                        font_color: Some(muted_color),
                        ..Default::default()
                    })
                    .build()
                    .finish();
                let chevron = Container::new(
                    ConstrainedBox::new(chevron.to_warpui_icon(Fill::from(muted_color)).finish())
                        .with_width(CHEVRON_SIZE)
                        .with_height(CHEVRON_SIZE)
                        .finish(),
                )
                .with_margin_right(CHEVRON_MARGIN_RIGHT)
                .finish();

                let mut container = Container::new(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_children([chevron, label])
                        .finish(),
                )
                .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
                .with_vertical_padding(ROW_VERTICAL_PADDING)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(ROW_CORNER_RADIUS)));
                if state.is_hovered() {
                    container = container
                        .with_background(theme.background().blend(&theme.surface_overlay_1()));
                }
                container.finish()
            },
        )
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(AgentPickerAction::ToggleNotInstalled);
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn render_row(
        &self,
        index: usize,
        def: &AgentDefinition,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let row = &self.rows[index];
        if row.is_installed {
            let clickable = Hoverable::new(row.mouse_state.clone(), |state| {
                self.render_row_contents(index, def, state.is_hovered(), app)
            })
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(AgentPickerAction::Select(index));
            })
            .with_cursor(Cursor::PointingHand)
            .finish();
            EventHandler::new(clickable)
                .on_right_mouse_down(move |ctx, _, _, _| {
                    ctx.dispatch_typed_action(WorkspaceAction::ShowCreateWorktreeModal {
                        agent_catalog_index: Some(index),
                    });
                    DispatchEventResult::StopPropagation
                })
                .finish()
        } else {
            self.render_row_contents(index, def, false, app)
        }
    }

    fn render_row_contents(
        &self,
        index: usize,
        def: &AgentDefinition,
        is_hovered: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let row = &self.rows[index];
        let is_selected = self.selected_index == Some(index);
        let muted_color = theme.disabled_text_color(theme.background()).into_solid();

        let icon = Container::new(
            ConstrainedBox::new(
                def.icon
                    .to_warpui_icon(Fill::from(def.cli_agent.brand_icon_color()))
                    .finish(),
            )
            .with_width(ICON_GLYPH_SIZE)
            .with_height(ICON_GLYPH_SIZE)
            .finish(),
        )
        .with_uniform_padding(ICON_CIRCLE_PADDING)
        .with_background_color(
            def.cli_agent
                .brand_color()
                .unwrap_or_else(|| theme.foreground().into_solid()),
        )
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
            ICON_GLYPH_SIZE / 2. + ICON_CIRCLE_PADDING,
        )))
        .with_margin_right(ROW_ICON_MARGIN_RIGHT)
        .finish();

        let mut name = appearance
            .ui_builder()
            .paragraph(def.display_name)
            .with_style(UiComponentStyles {
                font_size: Some(NAME_FONT_SIZE),
                ..Default::default()
            });
        if !row.is_installed {
            name = name.with_style(UiComponentStyles {
                font_color: Some(muted_color),
                ..Default::default()
            });
        }
        let first_line = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_children([icon, name.build().finish()]);

        let mut contents = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_children([first_line.finish()]);
        if !row.is_installed {
            contents.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .link(
                            "Install docs".into(),
                            Some(def.install_docs_url.to_string()),
                            None,
                            row.install_link_mouse_state.clone(),
                        )
                        .with_style(UiComponentStyles {
                            font_size: Some(DETAIL_FONT_SIZE),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .with_padding_left(ROW_DETAIL_INSET)
                .with_margin_top(2.)
                .finish(),
            );
        }
        let contents = contents.finish();

        let mut container = Container::new(contents)
            .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
            .with_vertical_padding(ROW_VERTICAL_PADDING)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(ROW_CORNER_RADIUS)));
        if row.is_installed && (is_selected || is_hovered) {
            container =
                container.with_background(theme.background().blend(&theme.surface_overlay_1()));
        }
        container.finish()
    }
}

impl Entity for AgentPickerView {
    type Event = AgentPickerViewEvent;
}

impl TypedActionView for AgentPickerView {
    type Action = AgentPickerAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AgentPickerAction::Up => self.select_previous(ctx),
            AgentPickerAction::Down => self.select_next(ctx),
            AgentPickerAction::Confirm => {
                if let Some(index) = self.selected_index {
                    self.launch(index, ctx);
                }
            }
            AgentPickerAction::Close => self.close(ctx),
            AgentPickerAction::Select(index) => self.launch(*index, ctx),
            AgentPickerAction::ToggleNotInstalled => {
                self.not_installed_expanded = !self.not_installed_expanded;
                ctx.notify();
            }
        }
    }
}

impl View for AgentPickerView {
    fn ui_name() -> &'static str {
        "AgentPickerView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let title = Align::new(
            appearance
                .ui_builder()
                .paragraph("Start an agent")
                .with_style(UiComponentStyles {
                    font_size: Some(TITLE_FONT_SIZE),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .finish();
        let subtitle = Align::new(
            appearance
                .ui_builder()
                .paragraph("Open a terminal running the agent of your choice")
                .with_style(UiComponentStyles {
                    font_size: Some(SUBTITLE_FONT_SIZE),
                    font_color: Some(theme.disabled_text_color(theme.background()).into_solid()),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_children([
                title,
                Container::new(subtitle)
                    .with_margin_top(4.)
                    .with_margin_bottom(16.)
                    .finish(),
            ]);
        let (installed, not_installed): (Vec<_>, Vec<_>) = catalog::agent_catalog()
            .iter()
            .enumerate()
            .partition(|(index, _)| self.rows[*index].is_installed);
        for (index, def) in installed {
            column.add_child(self.render_row(index, def, app));
        }
        if !not_installed.is_empty() {
            column.add_child(
                Container::new(self.render_not_installed_header(not_installed.len(), app))
                    .with_margin_top(NOT_INSTALLED_SECTION_MARGIN_TOP)
                    .finish(),
            );
            if self.not_installed_expanded {
                for (index, def) in not_installed {
                    column.add_child(self.render_row(index, def, app));
                }
            }
        }

        Align::new(
            ConstrainedBox::new(column.finish())
                .with_max_width(CONTENT_MAX_WIDTH)
                .finish(),
        )
        .finish()
    }
}

impl BackingView for AgentPickerView {
    type PaneHeaderOverflowMenuAction = ();
    type CustomAction = ();
    type AssociatedData = ();

    fn handle_pane_header_overflow_menu_action(
        &mut self,
        _action: &Self::PaneHeaderOverflowMenuAction,
        _ctx: &mut ViewContext<Self>,
    ) {
    }

    fn close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.emit(AgentPickerViewEvent::Pane(PaneEvent::Close));
    }

    fn focus_contents(&mut self, ctx: &mut ViewContext<Self>) {
        self.refresh_install_state(ctx);
        ctx.focus_self();
    }

    fn render_header_content(
        &self,
        _ctx: &view::HeaderRenderContext<'_>,
        _app: &AppContext,
    ) -> view::HeaderContent {
        view::HeaderContent::simple(AGENT_PICKER_PANE_TITLE)
    }

    fn set_focus_handle(&mut self, focus_handle: PaneFocusHandle, _ctx: &mut ViewContext<Self>) {
        self.focus_handle = Some(focus_handle);
    }
}

#[cfg(test)]
#[path = "agent_picker_view_tests.rs"]
mod tests;
