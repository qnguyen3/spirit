use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::Vector2F;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, ChildAnchor, ChildView, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, DispatchEventResult, Element, Empty, EventHandler,
    Fill, Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning,
    Padding, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, SavePosition, ScrollbarWidth,
    Shrinkable, Stack, Text,
};
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::components::UiComponent;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::registry::{ProjectRegistryEvent, ProjectRegistryModel};
use super::{ProjectId, ProjectKind, agent_status};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions};
use crate::menu::{Event as MenuEvent, Menu, MenuItem, MenuItemFields};
use crate::ui_components::buttons::close_button;
use crate::ui_components::icons::Icon;
use crate::util::time_format::format_approx_duration_from_now_utc;

const CARD_WIDTH: f32 = 240.;
const CARD_HEIGHT: f32 = 112.;
const CARD_GAP: f32 = 12.;
const CARD_RADIUS: Radius = Radius::Pixels(8.);
const TITLE_FONT_SIZE: f32 = 20.;
const CARD_NAME_FONT_SIZE: f32 = 14.;
const CARD_DETAIL_FONT_SIZE: f32 = 11.;
const CARD_CHIP_FONT_SIZE: f32 = 10.;
const CARD_ICON_SIZE: f32 = 16.;
const STATUS_DOT_SIZE: f32 = 8.;
const HEADER_MAX_WIDTH: f32 = (CARD_WIDTH + CARD_GAP) * COLUMNS as f32 - CARD_GAP;
const SCROLLBAR_WIDTH: ScrollbarWidth = ScrollbarWidth::Custom(8.);
const COLUMNS: usize = 4;

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;
    app.register_fixed_bindings([
        FixedBinding::new(
            "escape",
            OverviewAction::Close,
            id!("WorkspaceOverviewView"),
        ),
        FixedBinding::new(
            "enter",
            OverviewAction::Activate,
            id!("WorkspaceOverviewView"),
        ),
        FixedBinding::new(
            "left",
            OverviewAction::MoveLeft,
            id!("WorkspaceOverviewView"),
        ),
        FixedBinding::new(
            "right",
            OverviewAction::MoveRight,
            id!("WorkspaceOverviewView"),
        ),
        FixedBinding::new("up", OverviewAction::MoveUp, id!("WorkspaceOverviewView")),
        FixedBinding::new(
            "down",
            OverviewAction::MoveDown,
            id!("WorkspaceOverviewView"),
        ),
    ]);
}

#[derive(Debug, Clone, Copy)]
pub enum OverviewAction {
    Close,
    Activate,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    Select(usize),
    ShowCardMenu {
        index: usize,
        project_id: ProjectId,
        position: Vector2F,
    },
    ShowHomeCardMenu {
        index: usize,
        position: Vector2F,
    },
    CloseHome,
    Open(ProjectId),
    StartRename(ProjectId),
    Remove(ProjectId),
    Reveal(ProjectId),
}

#[derive(Debug, Clone)]
pub enum OverviewEvent {
    Close,
    ActivateHome,
    OpenProject(ProjectId),
    NewWorkspace,
    RemoveProject(ProjectId),
    CloseHome,
    RevealProject(ProjectId),
    MissingRoot(ProjectId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardKind {
    Home,
    Project(ProjectId),
    New,
}

pub struct WorkspaceOverviewView {
    cards: Vec<CardKind>,
    mouse_states: HashMap<usize, MouseStateHandle>,
    close_mouse_state: MouseStateHandle,
    scroll_state: ClippedScrollStateHandle,
    selected_index: usize,
    renaming: Option<ProjectId>,
    rename_editor: ViewHandle<EditorView>,
    open_project_ids: Vec<ProjectId>,
    home_open: bool,
    card_menu: ViewHandle<Menu<OverviewAction>>,
    card_menu_offset: Option<Vector2F>,
    view_position_id: String,
}

impl Entity for WorkspaceOverviewView {
    type Event = OverviewEvent;
}

impl TypedActionView for WorkspaceOverviewView {
    type Action = OverviewAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            OverviewAction::Close => ctx.emit(OverviewEvent::Close),
            OverviewAction::Activate => self.activate_selected(ctx),
            OverviewAction::MoveLeft => self.move_selection(-1, ctx),
            OverviewAction::MoveRight => self.move_selection(1, ctx),
            OverviewAction::MoveUp => self.move_selection(-(COLUMNS as isize), ctx),
            OverviewAction::MoveDown => self.move_selection(COLUMNS as isize, ctx),
            OverviewAction::Select(index) => {
                self.selected_index = *index;
                self.activate_selected(ctx);
            }
            OverviewAction::ShowCardMenu {
                index,
                project_id,
                position,
            } => self.show_card_menu(*index, *project_id, *position, ctx),
            OverviewAction::ShowHomeCardMenu { index, position } => {
                self.show_home_card_menu(*index, *position, ctx)
            }
            OverviewAction::CloseHome => ctx.emit(OverviewEvent::CloseHome),
            OverviewAction::Open(project_id) => ctx.emit(OverviewEvent::OpenProject(*project_id)),
            OverviewAction::StartRename(project_id) => self.start_rename(*project_id, ctx),
            OverviewAction::Remove(project_id) => {
                ctx.emit(OverviewEvent::RemoveProject(*project_id))
            }
            OverviewAction::Reveal(project_id) => {
                ctx.emit(OverviewEvent::RevealProject(*project_id))
            }
        }
    }
}

impl WorkspaceOverviewView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let rename_editor = ctx.add_typed_action_view(|ctx| {
            EditorView::single_line(SingleLineEditorOptions::default(), ctx)
        });
        ctx.subscribe_to_view(&rename_editor, |me, _, event, ctx| match event {
            EditorEvent::Enter => me.commit_rename(ctx),
            EditorEvent::Escape => {
                me.renaming = None;
                ctx.notify();
            }
            _ => {}
        });

        ctx.subscribe_to_model(
            &ProjectRegistryModel::handle(ctx),
            |me: &mut Self, _, _event: &ProjectRegistryEvent, ctx| {
                me.rebuild_cards(ctx);
            },
        );

        let card_menu = ctx.add_typed_action_view(|_| {
            Menu::new()
                .with_drop_shadow()
                .prevent_interaction_with_other_elements()
        });
        ctx.subscribe_to_view(&card_menu, |me, _, event, ctx| {
            if let MenuEvent::Close { .. } = event {
                me.card_menu_offset = None;
                ctx.notify();
            }
        });

        let mut view = Self {
            cards: Vec::new(),
            mouse_states: HashMap::new(),
            close_mouse_state: MouseStateHandle::default(),
            scroll_state: ClippedScrollStateHandle::default(),
            selected_index: 0,
            renaming: None,
            rename_editor,
            open_project_ids: Vec::new(),
            home_open: true,
            card_menu,
            card_menu_offset: None,
            view_position_id: format!("workspace_overview_view_{}", ctx.view_id()),
        };
        view.rebuild_cards(ctx);
        view
    }

    pub fn set_open_projects(&mut self, open: Vec<ProjectId>, ctx: &mut ViewContext<Self>) {
        self.open_project_ids = open;
        ctx.notify();
    }

    pub fn set_home_open(&mut self, home_open: bool, ctx: &mut ViewContext<Self>) {
        if self.home_open == home_open {
            return;
        }
        self.home_open = home_open;
        self.rebuild_cards(ctx);
    }

    pub fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        self.rebuild_cards(ctx);
    }

    fn rebuild_cards(&mut self, ctx: &mut ViewContext<Self>) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let mut cards = Vec::new();
        if self.home_open {
            cards.push(CardKind::Home);
        }
        cards.extend(
            registry
                .projects_mru()
                .into_iter()
                .map(|project| CardKind::Project(project.id)),
        );
        cards.push(CardKind::New);

        self.mouse_states = (0..cards.len())
            .map(|index| {
                (
                    index,
                    self.mouse_states.get(&index).cloned().unwrap_or_default(),
                )
            })
            .collect();
        self.selected_index = self.selected_index.min(cards.len().saturating_sub(1));
        self.cards = cards;
        ctx.notify();
    }

    fn move_selection(&mut self, delta: isize, ctx: &mut ViewContext<Self>) {
        if self.cards.is_empty() {
            return;
        }
        let len = self.cards.len() as isize;
        let next = (self.selected_index as isize + delta).clamp(0, len - 1);
        self.selected_index = next as usize;
        ctx.notify();
    }

    fn activate_selected(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(card) = self.cards.get(self.selected_index).copied() else {
            return;
        };
        match card {
            CardKind::Home => ctx.emit(OverviewEvent::ActivateHome),
            CardKind::New => ctx.emit(OverviewEvent::NewWorkspace),
            CardKind::Project(project_id) => {
                let missing = ProjectRegistryModel::as_ref(ctx)
                    .project(project_id)
                    .is_some_and(|project| !project.root_path.exists());
                if missing {
                    ctx.emit(OverviewEvent::MissingRoot(project_id));
                } else {
                    ctx.emit(OverviewEvent::OpenProject(project_id));
                }
            }
        }
    }

    fn show_card_menu(
        &mut self,
        index: usize,
        project_id: ProjectId,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        self.selected_index = index;
        let items = vec![
            MenuItemFields::new("Open")
                .with_on_select_action(OverviewAction::Open(project_id))
                .into_item(),
            MenuItemFields::new("Rename\u{2026}")
                .with_on_select_action(OverviewAction::StartRename(project_id))
                .into_item(),
            MenuItemFields::new(reveal_label())
                .with_on_select_action(OverviewAction::Reveal(project_id))
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new("Remove from Spirit\u{2026}")
                .with_on_select_action(OverviewAction::Remove(project_id))
                .into_item(),
        ];
        self.card_menu
            .update(ctx, |menu, ctx| menu.set_items(items, ctx));
        let view_origin = ctx
            .element_position_by_id(&self.view_position_id)
            .map(|bounds| bounds.origin())
            .unwrap_or_default();
        self.card_menu_offset = Some(position - view_origin);
        ctx.focus(&self.card_menu);
        ctx.notify();
    }

    fn show_home_card_menu(
        &mut self,
        index: usize,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        self.selected_index = index;
        let items = vec![
            MenuItemFields::new("Open")
                .with_on_select_action(OverviewAction::Activate)
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new("Close Home\u{2026}")
                .with_on_select_action(OverviewAction::CloseHome)
                .into_item(),
        ];
        self.card_menu
            .update(ctx, |menu, ctx| menu.set_items(items, ctx));
        let view_origin = ctx
            .element_position_by_id(&self.view_position_id)
            .map(|bounds| bounds.origin())
            .unwrap_or_default();
        self.card_menu_offset = Some(position - view_origin);
        ctx.focus(&self.card_menu);
        ctx.notify();
    }

    fn start_rename(&mut self, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
        let Some(name) = ProjectRegistryModel::as_ref(ctx)
            .project(project_id)
            .map(|project| project.display_name.clone())
        else {
            return;
        };
        self.renaming = Some(project_id);
        self.rename_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(&name, ctx);
        });
        ctx.focus(&self.rename_editor);
        ctx.notify();
    }

    fn commit_rename(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(project_id) = self.renaming.take() else {
            return;
        };
        let name = self.rename_editor.as_ref(ctx).buffer_text(ctx);
        let name = name.trim().to_owned();
        if !name.is_empty() {
            ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                registry.rename_project(project_id, name, ctx);
            });
        }
        ctx.notify();
    }

    fn card_content(&self, card: CardKind, app: &AppContext) -> CardContent {
        match card {
            CardKind::Home => CardContent {
                icon: Icon::Terminal,
                name: "Home".to_owned(),
                detail: "Free-floating terminal tabs".to_owned(),
                chips: Vec::new(),
                status_dot: None,
                last_opened: None,
            },
            CardKind::New => CardContent {
                icon: Icon::Plus,
                name: "New Workspace".to_owned(),
                detail: "Open, clone, or create".to_owned(),
                chips: Vec::new(),
                status_dot: None,
                last_opened: None,
            },
            CardKind::Project(project_id) => {
                let registry = ProjectRegistryModel::as_ref(app);
                let Some(project) = registry.project(project_id) else {
                    return CardContent {
                        icon: Icon::Folder,
                        name: "Unknown".to_owned(),
                        detail: String::new(),
                        chips: Vec::new(),
                        status_dot: None,
                        last_opened: None,
                    };
                };
                let theme = Appearance::as_ref(app).theme();

                let mut chips = Vec::new();
                if !project.root_path.exists() {
                    chips.push(CardChip {
                        label: "missing".to_owned(),
                        emphasized: true,
                    });
                } else if self.open_project_ids.contains(&project_id) {
                    chips.push(CardChip {
                        label: "open".to_owned(),
                        emphasized: false,
                    });
                }
                if let Some(branch) = &project.primary_branch {
                    chips.push(CardChip {
                        label: branch.clone(),
                        emphasized: false,
                    });
                }
                let worktrees = registry.linked_worktree_count(project_id);
                if worktrees > 0 {
                    let label = if worktrees == 1 {
                        "1 worktree".to_owned()
                    } else {
                        format!("{worktrees} worktrees")
                    };
                    chips.push(CardChip {
                        label,
                        emphasized: false,
                    });
                }
                let (working, needs_attention) = agent_status::project_counts(project_id, app);
                if needs_attention > 0 {
                    let label = if needs_attention == 1 {
                        "1 agent needs attention".to_owned()
                    } else {
                        format!("{needs_attention} agents need attention")
                    };
                    chips.push(CardChip {
                        label,
                        emphasized: true,
                    });
                } else if working > 0 {
                    let label = if working == 1 {
                        "1 agent working".to_owned()
                    } else {
                        format!("{working} agents working")
                    };
                    chips.push(CardChip {
                        label,
                        emphasized: false,
                    });
                }

                let status_dot = match agent_status::summarize_project(project_id, app) {
                    agent_status::WorktreeAgentSummary::NeedsAttention => {
                        Some(theme.ansi_fg_yellow())
                    }
                    agent_status::WorktreeAgentSummary::Working => Some(theme.ansi_fg_green()),
                    agent_status::WorktreeAgentSummary::None => None,
                };

                let last_opened = DateTime::<Utc>::from_timestamp(project.last_opened_ts, 0)
                    .filter(|_| project.last_opened_ts > 0)
                    .map(|opened| {
                        format!("Opened {}", format_approx_duration_from_now_utc(opened))
                    });

                let kind_icon = match project.kind {
                    ProjectKind::Git => Icon::GitBranch,
                    ProjectKind::Folder => Icon::Folder,
                };

                CardContent {
                    icon: kind_icon,
                    name: project.display_name.clone(),
                    detail: middle_truncate(&project.root_path, 30),
                    chips,
                    status_dot,
                    last_opened,
                }
            }
        }
    }

    fn render_card(&self, index: usize, card: CardKind, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let is_selected = index == self.selected_index;
        let Some(mouse_state) = self.mouse_states.get(&index).cloned() else {
            return Empty::new().finish();
        };

        let content = self.card_content(card, app);
        let renaming_this = matches!(card, CardKind::Project(id) if self.renaming == Some(id));
        let rename_editor = renaming_this.then(|| self.rename_editor.clone());

        let ui_font = appearance.ui_font_family();
        let mono_font = appearance.monospace_font_family();
        let name_color = theme.foreground();
        let sub_color = theme.sub_text_color(theme.background());
        let chip_background = internal_colors::fg_overlay_2(theme);
        let missing_chip_color = theme.ansi_fg_yellow();
        let background = if is_selected {
            internal_colors::fg_overlay_2(theme)
        } else {
            internal_colors::fg_overlay_1(theme)
        };
        let hovered_background = internal_colors::fg_overlay_2(theme);

        let card_element = Hoverable::new(mouse_state, move |state| {
            let name_element: Box<dyn Element> = match &rename_editor {
                Some(editor) => ChildView::new(editor).finish(),
                None => Text::new_inline(content.name.clone(), ui_font, CARD_NAME_FONT_SIZE)
                    .with_clip(ClipConfig::ellipsis())
                    .with_color(name_color.into())
                    .finish(),
            };

            let mut name_row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(
                    ConstrainedBox::new(content.icon.to_warpui_icon(sub_color).finish())
                        .with_width(CARD_ICON_SIZE)
                        .with_height(CARD_ICON_SIZE)
                        .finish(),
                )
                .with_child(Shrinkable::new(1., name_element).finish());
            if let Some(dot_color) = content.status_dot {
                name_row.add_child(
                    ConstrainedBox::new(
                        Container::new(Empty::new().finish())
                            .with_background(Fill::from(dot_color))
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                                STATUS_DOT_SIZE / 2.,
                            )))
                            .finish(),
                    )
                    .with_width(STATUS_DOT_SIZE)
                    .with_height(STATUS_DOT_SIZE)
                    .finish(),
                );
            }

            let mut column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(6.)
                .with_child(name_row.finish());

            if !content.detail.is_empty() {
                column.add_child(
                    Text::new_inline(content.detail.clone(), mono_font, CARD_DETAIL_FONT_SIZE)
                        .with_clip(ClipConfig::ellipsis())
                        .with_color(sub_color.into())
                        .finish(),
                );
            }

            if !content.chips.is_empty() {
                let mut chips_row = Flex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.);
                for chip in &content.chips {
                    let label_color = if chip.emphasized {
                        missing_chip_color
                    } else {
                        sub_color.into()
                    };
                    chips_row.add_child(
                        Container::new(
                            Text::new_inline(chip.label.clone(), ui_font, CARD_CHIP_FONT_SIZE)
                                .with_clip(ClipConfig::ellipsis())
                                .with_color(label_color)
                                .finish(),
                        )
                        .with_background(chip_background)
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                        .with_horizontal_padding(6.)
                        .with_vertical_padding(2.)
                        .finish(),
                    );
                }
                column.add_child(chips_row.finish());
            }

            if let Some(last_opened) = &content.last_opened {
                column.add_child(
                    Text::new_inline(last_opened.clone(), ui_font, CARD_CHIP_FONT_SIZE)
                        .with_color(sub_color.into())
                        .finish(),
                );
            }

            ConstrainedBox::new(
                Container::new(column.finish())
                    .with_padding(Padding::uniform(12.))
                    .with_background(if state.is_hovered() {
                        hovered_background
                    } else {
                        background
                    })
                    .with_corner_radius(CornerRadius::with_all(CARD_RADIUS))
                    .finish(),
            )
            .with_width(CARD_WIDTH)
            .with_height(CARD_HEIGHT)
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(OverviewAction::Select(index));
        })
        .finish();

        match card {
            CardKind::Project(project_id) => EventHandler::new(card_element)
                .on_right_mouse_down(move |ctx, _, position, _| {
                    ctx.dispatch_typed_action(OverviewAction::ShowCardMenu {
                        index,
                        project_id,
                        position,
                    });
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            CardKind::Home => EventHandler::new(card_element)
                .on_right_mouse_down(move |ctx, _, position, _| {
                    ctx.dispatch_typed_action(OverviewAction::ShowHomeCardMenu { index, position });
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            CardKind::New => card_element,
        }
    }
}

struct CardContent {
    icon: Icon,
    name: String,
    detail: String,
    chips: Vec<CardChip>,
    status_dot: Option<ColorU>,
    last_opened: Option<String>,
}

struct CardChip {
    label: String,
    emphasized: bool,
}

impl View for WorkspaceOverviewView {
    fn ui_name() -> &'static str {
        "WorkspaceOverviewView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let mut grid = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(CARD_GAP);
        for (row_index, chunk) in self.cards.chunks(COLUMNS).enumerate() {
            let offset = row_index * COLUMNS;
            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(CARD_GAP);
            for (column, card) in chunk.iter().enumerate() {
                row.add_child(self.render_card(offset + column, *card, app));
            }
            grid.add_child(row.finish());
        }
        let scrollable_grid = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            grid.finish(),
            SCROLLBAR_WIDTH,
            theme.disabled_text_color(theme.background()).into(),
            theme.main_text_color(theme.background()).into(),
            Fill::None,
        )
        .finish();

        let header = ConstrainedBox::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_children([
                    Text::new_inline(
                        "Workspaces".to_owned(),
                        appearance.ui_font_family(),
                        TITLE_FONT_SIZE,
                    )
                    .with_color(theme.foreground().into())
                    .finish(),
                    close_button(appearance, self.close_mouse_state.clone())
                        .build()
                        .on_click(|ctx, _, _| ctx.dispatch_typed_action(OverviewAction::Close))
                        .with_cursor(Cursor::PointingHand)
                        .finish(),
                ])
                .finish(),
        )
        .with_width(HEADER_MAX_WIDTH)
        .finish();

        let content = ConstrainedBox::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(20.)
                .with_children([header, scrollable_grid])
                .finish(),
        )
        .with_width(HEADER_MAX_WIDTH)
        .finish();

        let mut stack = Stack::new();
        ParentElement::add_child(
            &mut stack,
            SavePosition::new(
                Container::new(Align::new(content).finish())
                    .with_background(theme.background())
                    .finish(),
                &self.view_position_id,
            )
            .finish(),
        );
        if let Some(offset) = self.card_menu_offset {
            stack.add_positioned_child(
                ChildView::new(&self.card_menu).finish(),
                OffsetPositioning::offset_from_parent(
                    offset,
                    ParentOffsetBounds::WindowByPosition,
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }
        stack.finish()
    }
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Reveal in Explorer"
    } else {
        "Reveal in file manager"
    }
}

fn middle_truncate(path: &Path, max_len: usize) -> String {
    let text = path.to_string_lossy().to_string();
    if text.chars().count() <= max_len {
        return text;
    }
    let keep = max_len.saturating_sub(1) / 2;
    let head: String = text.chars().take(keep).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}\u{2026}{tail}")
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
