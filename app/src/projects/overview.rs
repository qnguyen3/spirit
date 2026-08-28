use std::collections::HashMap;
use std::path::PathBuf;

use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Element, Empty, EventHandler, Flex, Hoverable, MainAxisSize,
    MouseStateHandle, Padding, ParentElement, Radius, Stack, Text,
};
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::ProjectId;
use super::registry::{ProjectRegistryEvent, ProjectRegistryModel};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions};
use crate::menu::{Event as MenuEvent, Menu, MenuItem, MenuItemFields};

const CARD_WIDTH: f32 = 240.;
const CARD_HEIGHT: f32 = 112.;
const CARD_GAP: f32 = 12.;
const CARD_RADIUS: Radius = Radius::Pixels(8.);
const TITLE_FONT_SIZE: f32 = 20.;
const CARD_NAME_FONT_SIZE: f32 = 14.;
const CARD_DETAIL_FONT_SIZE: f32 = 11.;
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
    ShowCardMenu { index: usize, project_id: ProjectId },
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
    selected_index: usize,
    renaming: Option<ProjectId>,
    rename_editor: ViewHandle<EditorView>,
    open_project_ids: Vec<ProjectId>,
    card_menu: ViewHandle<Menu<OverviewAction>>,
    show_card_menu: bool,
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
            OverviewAction::ShowCardMenu { index, project_id } => {
                self.show_card_menu(*index, *project_id, ctx)
            }
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
                me.show_card_menu = false;
                ctx.notify();
            }
        });

        let mut view = Self {
            cards: Vec::new(),
            mouse_states: HashMap::new(),
            selected_index: 0,
            renaming: None,
            rename_editor,
            open_project_ids: Vec::new(),
            card_menu,
            show_card_menu: false,
        };
        view.rebuild_cards(ctx);
        view
    }

    pub fn set_open_projects(&mut self, open: Vec<ProjectId>, ctx: &mut ViewContext<Self>) {
        self.open_project_ids = open;
        ctx.notify();
    }

    pub fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        self.rebuild_cards(ctx);
    }

    fn rebuild_cards(&mut self, ctx: &mut ViewContext<Self>) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let mut cards = vec![CardKind::Home];
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

    fn show_card_menu(&mut self, index: usize, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
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
        self.show_card_menu = true;
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

    fn render_card(&self, index: usize, card: CardKind, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let is_selected = index == self.selected_index;
        let mouse_state = self.mouse_states.get(&index).cloned().unwrap_or_default();

        let (name, detail, badge) = match card {
            CardKind::Home => (
                "Home".to_owned(),
                "Free-floating terminal tabs".to_owned(),
                None,
            ),
            CardKind::New => ("+ New Workspace".to_owned(), String::new(), None),
            CardKind::Project(project_id) => {
                let registry = ProjectRegistryModel::as_ref(app);
                match registry.project(project_id) {
                    Some(project) => {
                        let mut badges = Vec::new();
                        if !project.root_path.exists() {
                            badges.push("missing".to_owned());
                        } else if self.open_project_ids.contains(&project_id) {
                            badges.push("open".to_owned());
                        }
                        if let Some(branch) = &project.primary_branch {
                            badges.push(branch.clone());
                        }
                        let worktrees = registry.linked_worktree_count(project_id);
                        if worktrees > 0 {
                            badges.push(format!("{worktrees} worktrees"));
                        }
                        (
                            project.display_name.clone(),
                            middle_truncate(&project.root_path, 34),
                            Some(badges.join(" · ")),
                        )
                    }
                    None => ("Unknown".to_owned(), String::new(), None),
                }
            }
        };

        let renaming_this = matches!(card, CardKind::Project(id) if self.renaming == Some(id));
        let name_element: Box<dyn Element> = if renaming_this {
            ChildView::new(&self.rename_editor).finish()
        } else {
            Text::new_inline(name, appearance.ui_font_family(), CARD_NAME_FONT_SIZE)
                .with_color(theme.foreground().into())
                .finish()
        };

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(6.)
            .with_child(name_element);

        if !detail.is_empty() {
            column.add_child(
                Text::new_inline(
                    detail,
                    appearance.monospace_font_family(),
                    CARD_DETAIL_FONT_SIZE,
                )
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
            );
        }
        if let Some(badge) = badge.filter(|badge| !badge.is_empty()) {
            column.add_child(
                Text::new_inline(badge, appearance.ui_font_family(), CARD_DETAIL_FONT_SIZE)
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .finish(),
            );
        }

        let background = if is_selected {
            internal_colors::fg_overlay_2(theme)
        } else {
            internal_colors::fg_overlay_1(theme)
        };
        let hovered_background = internal_colors::fg_overlay_2(theme);
        let card_body = Hoverable::new(mouse_state, move |state| {
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
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

        let mut stack = Stack::new();
        stack.add_child(card_body);
        stack.add_child(
            ConstrainedBox::new(
                Container::new(column.finish())
                    .with_padding(Padding::uniform(12.))
                    .finish(),
            )
            .with_width(CARD_WIDTH)
            .with_height(CARD_HEIGHT)
            .finish(),
        );

        match card {
            CardKind::Project(project_id) => EventHandler::new(stack.finish())
                .on_right_mouse_down(move |ctx, _, _, _| {
                    ctx.dispatch_typed_action(OverviewAction::ShowCardMenu { index, project_id });
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            _ => stack.finish(),
        }
    }
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
        for chunk in self.cards.chunks(COLUMNS) {
            let offset = self
                .cards
                .iter()
                .position(|card| Some(card) == chunk.first())
                .unwrap_or(0);
            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(CARD_GAP);
            for (column, card) in chunk.iter().enumerate() {
                row.add_child(self.render_card(offset + column, *card, app));
            }
            grid.add_child(row.finish());
        }

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(20.)
            .with_children([
                Text::new_inline(
                    "Workspaces".to_owned(),
                    appearance.ui_font_family(),
                    TITLE_FONT_SIZE,
                )
                .with_color(theme.foreground().into())
                .finish(),
                grid.finish(),
            ])
            .finish();

        let mut stack = Stack::new();
        ParentElement::add_child(
            &mut stack,
            Container::new(Align::new(content).finish())
                .with_background(theme.background())
                .finish(),
        );
        if self.show_card_menu {
            ParentElement::add_child(&mut stack, ChildView::new(&self.card_menu).finish());
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

fn middle_truncate(path: &PathBuf, max_len: usize) -> String {
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
