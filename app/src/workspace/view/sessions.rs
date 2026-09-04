use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use settings::Setting;
use warp_errors::report_if_error;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ChildAnchor, ChildView, Clipped, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Element, Empty, Expanded, Fill, Flex, Hoverable, List, ListState,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning, ParentElement,
    PositionedElementAnchor, PositionedElementOffsetBounds, Radius, SavePosition,
    ScrollStateHandle, Scrollable, ScrollableElement, ScrollbarWidth, Shrinkable, Stack, Text,
    Wrap,
};
use warpui::fonts::{Properties, Weight};
use warpui::geometry::vector::vec2f;
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::button::{ButtonVariant, TextAndIcon, TextAndIconAlignment};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Entity, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::menu::{Event as MenuEvent, Menu, MenuItem, MenuItemFields};
use crate::projects::registry::{ProjectRegistryEvent, ProjectRegistryModel};
use crate::projects::{ProjectId, WorktreeId};
use crate::settings::CodeSettings;
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_session_history::{
    AgentSession, AgentSessionHistoryModel, PreviewRole, SUPPORTED_AGENTS, ScanState,
    SessionFilter, SessionGroup, SessionGroupResult, SessionLimit, SessionSort, filter_sessions,
    group_sessions, path_contains,
};
use crate::terminal::cli_agent_sessions::{CLIAgentSessionsModel, CLIAgentSessionsModelEvent};
use crate::ui_components::buttons::icon_button;
use crate::ui_components::icons::Icon;
use crate::util::time_format::format_approx_duration_from_now_utc;
use crate::workspace::view::left_panel::LeftPanelEvent;

pub const SESSIONS_PANEL_HEADER_POSITION_ID: &str = "sessions_panel:header";

const WORKTREE_ANCHOR_ID: &str = "sessions_panel:filter:worktree";
const AGENTS_ANCHOR_ID: &str = "sessions_panel:filter:agents";
const OPTIONS_ANCHOR_ID: &str = "sessions_panel:filter:options";

const FILTER_MENU_WIDTH: f32 = 216.;
const WORKTREE_BUTTON_MAX_WIDTH: f32 = 140.;
const AGENTS_BUTTON_MAX_WIDTH: f32 = 132.;
const MAX_SESSION_ACTIONS: usize = 12;
const ROW_HORIZONTAL_PADDING: f32 = 10.;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterMenu {
    Worktree,
    Agents,
    Options,
}

impl FilterMenu {
    fn anchor_id(self) -> &'static str {
        match self {
            FilterMenu::Worktree => WORKTREE_ANCHOR_ID,
            FilterMenu::Agents => AGENTS_ANCHOR_ID,
            FilterMenu::Options => OPTIONS_ANCHOR_ID,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SessionsAction {
    ToggleFilterMenu(FilterMenu),
    SetWorktree(Option<WorktreeId>),
    SetSort(SessionSort),
    SetGroup(SessionGroup),
    SetLimit(SessionLimit),
    ToggleAgent(CLIAgent),
    EnableAllAgents,
    ToggleGroup(String),
    ToggleSession(String),
    ToggleHideEmpty,
    Refresh,
    Resume(String),
    CopyCommand(String),
    CopyPrompt(String),
    CopyId(String),
    CopyPath(String),
    OpenLog(String),
    RevealLog(String),
    JumpOriginal(warpui::EntityId),
    Delete(String),
}

#[derive(Clone)]
struct SessionWorktree {
    id: WorktreeId,
    name: String,
    path: std::path::PathBuf,
}

#[derive(Clone)]
enum ListItem {
    Group(SessionGroupResult),
    Session(Box<AgentSession>),
}

pub struct SessionsView {
    history: ModelHandle<AgentSessionHistoryModel>,
    query_editor: ViewHandle<EditorView>,
    filter_menu: ViewHandle<Menu<SessionsAction>>,
    open_filter_menu: Option<FilterMenu>,
    list_state: ListState<()>,
    scroll_state: ScrollStateHandle,
    items: Arc<Vec<ListItem>>,
    session_count: usize,
    sort: SessionSort,
    group: SessionGroup,
    limit: SessionLimit,
    hide_empty: bool,
    enabled_agents: HashSet<CLIAgent>,
    collapsed_groups: HashSet<String>,
    expanded_sessions: HashSet<String>,
    row_mouse_states: Vec<MouseStateHandle>,
    action_mouse_states: HashMap<String, Vec<MouseStateHandle>>,
    worktree_button_mouse_state: MouseStateHandle,
    agents_button_mouse_state: MouseStateHandle,
    options_button_mouse_state: MouseStateHandle,
    workspace_paths: Vec<std::path::PathBuf>,
    project_id: Option<ProjectId>,
    worktrees: Vec<SessionWorktree>,
    selected_worktree: Option<WorktreeId>,
}

impl SessionsView {
    pub fn new(project_id: Option<ProjectId>, ctx: &mut ViewContext<Self>) -> Self {
        let (sort, group, limit, hide_empty, enabled_agents) = {
            let settings = CodeSettings::as_ref(ctx);
            let sort = match settings.agent_session_history_sort.value().as_str() {
                "created" => SessionSort::Created,
                _ => SessionSort::Updated,
            };
            let group = match settings.agent_session_history_group.value().as_str() {
                "folder" => SessionGroup::Folder,
                "agent" => SessionGroup::Agent,
                _ => SessionGroup::Project,
            };
            let limit = match settings.agent_session_history_limit.value().as_str() {
                "500" => SessionLimit::FiveHundred,
                "1000" => SessionLimit::OneThousand,
                "all" => SessionLimit::Unlimited,
                _ => SessionLimit::TwoHundredFifty,
            };
            let enabled_agents = if settings.agent_session_history_agents.value() == "*" {
                SUPPORTED_AGENTS.into_iter().collect()
            } else {
                settings
                    .agent_session_history_agents
                    .value()
                    .split(',')
                    .map(CLIAgent::from_serialized_name)
                    .filter(|agent| SUPPORTED_AGENTS.contains(agent))
                    .collect()
            };
            (
                sort,
                group,
                limit,
                *settings.agent_session_history_hide_empty.value(),
                enabled_agents,
            )
        };
        let history = AgentSessionHistoryModel::handle(ctx);
        let query_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(14.), appearance),
                    select_all_on_focus: true,
                    clear_selections_on_blur: true,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    propagate_horizontal_navigation_keys: PropagateHorizontalNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("Search sessions, repo:…, path:…", ctx);
            editor
        });
        ctx.subscribe_to_view(&query_editor, |view, _, event, ctx| {
            if matches!(event, EditorEvent::Edited(_)) {
                view.rebuild_items(ctx);
            }
        });
        let filter_menu = ctx.add_typed_action_view(|ctx| {
            let theme = Appearance::as_ref(ctx).theme();
            Menu::<SessionsAction>::new()
                .with_width(FILTER_MENU_WIDTH)
                .with_border(Border::all(1.).with_border_fill(theme.outline()))
                .with_drop_shadow()
                .prevent_interaction_with_other_elements()
        });
        ctx.subscribe_to_view(&filter_menu, |view, _, event, ctx| match event {
            MenuEvent::Close { via_select_item } => {
                let keeps_menu_open =
                    *via_select_item && view.open_filter_menu == Some(FilterMenu::Agents);
                if !keeps_menu_open {
                    view.open_filter_menu = None;
                    ctx.notify();
                }
            }
            MenuEvent::ItemSelected | MenuEvent::ItemHovered => {}
        });
        ctx.subscribe_to_model(&history, |view, _, _, ctx| view.rebuild_items(ctx));
        ctx.subscribe_to_model(
            &ProjectRegistryModel::handle(ctx),
            |view: &mut Self, _, _: &ProjectRegistryEvent, ctx| view.reload_worktrees(ctx),
        );
        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            |view, _, event, ctx| {
                match event {
                    CLIAgentSessionsModelEvent::Ended { .. } => {
                        view.history
                            .update(ctx, |history, ctx| history.refresh(true, ctx));
                    }
                    CLIAgentSessionsModelEvent::SessionUpdated { .. } => {
                        view.history
                            .update(ctx, |history, ctx| history.refresh(false, ctx));
                    }
                    CLIAgentSessionsModelEvent::Started { .. }
                    | CLIAgentSessionsModelEvent::StatusChanged { .. }
                    | CLIAgentSessionsModelEvent::InputSessionChanged { .. } => {}
                }
                ctx.notify();
            },
        );

        history.update(ctx, |history, ctx| history.refresh(false, ctx));

        let handle = ctx.handle();
        let list_state = ListState::new(move |index, _, app| {
            let Some(view) = handle.upgrade(app) else {
                return Empty::new().finish();
            };
            let view = view.as_ref(app);
            match view.items.get(index) {
                Some(ListItem::Group(group)) => {
                    view.render_group(group, index, Appearance::as_ref(app))
                }
                Some(ListItem::Session(session)) => {
                    view.render_session(session, index, Appearance::as_ref(app), app)
                }
                None => Empty::new().finish(),
            }
        });
        let mut view = Self {
            history,
            query_editor,
            filter_menu,
            open_filter_menu: None,
            list_state,
            scroll_state: Arc::new(Mutex::new(Default::default())),
            items: Arc::new(Vec::new()),
            session_count: 0,
            sort,
            group,
            limit,
            hide_empty,
            enabled_agents,
            collapsed_groups: HashSet::new(),
            expanded_sessions: HashSet::new(),
            row_mouse_states: Vec::new(),
            action_mouse_states: HashMap::new(),
            worktree_button_mouse_state: MouseStateHandle::default(),
            agents_button_mouse_state: MouseStateHandle::default(),
            options_button_mouse_state: MouseStateHandle::default(),
            workspace_paths: Vec::new(),
            project_id,
            worktrees: Vec::new(),
            selected_worktree: None,
        };
        view.reload_worktrees(ctx);
        let initial_limit = view.limit;
        view.history
            .update(ctx, |history, ctx| history.set_limit(initial_limit, ctx));
        view
    }

    fn persist_view_options(&self, ctx: &mut ViewContext<Self>) {
        let sort = match self.sort {
            SessionSort::Updated => "updated",
            SessionSort::Created => "created",
        };
        let group = match self.group {
            SessionGroup::Project => "project",
            SessionGroup::Folder => "folder",
            SessionGroup::Agent => "agent",
        };
        let limit = match self.limit {
            SessionLimit::TwoHundredFifty => "250",
            SessionLimit::FiveHundred => "500",
            SessionLimit::OneThousand => "1000",
            SessionLimit::Unlimited => "all",
        };
        let mut agents = self
            .enabled_agents
            .iter()
            .map(CLIAgent::to_serialized_name)
            .collect::<Vec<_>>();
        agents.sort();
        let agents = if agents.len() == SUPPORTED_AGENTS.len() {
            "*".to_owned()
        } else {
            agents.join(",")
        };
        CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(
                settings
                    .agent_session_history_sort
                    .set_value(sort.to_owned(), ctx)
            );
            report_if_error!(
                settings
                    .agent_session_history_group
                    .set_value(group.to_owned(), ctx)
            );
            report_if_error!(
                settings
                    .agent_session_history_limit
                    .set_value(limit.to_owned(), ctx)
            );
            report_if_error!(
                settings
                    .agent_session_history_hide_empty
                    .set_value(self.hide_empty, ctx)
            );
            report_if_error!(settings.agent_session_history_agents.set_value(agents, ctx));
        });
    }

    pub fn focus_search(&self, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.query_editor);
    }

    pub fn set_workspace_paths(
        &mut self,
        workspace_paths: Vec<std::path::PathBuf>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.workspace_paths = workspace_paths;
        self.rebuild_items(ctx);
    }

    fn reload_worktrees(&mut self, ctx: &mut ViewContext<Self>) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        self.worktrees = self
            .project_id
            .map(|project_id| {
                registry
                    .worktrees_for_project(project_id)
                    .into_iter()
                    .filter_map(|worktree| {
                        registry
                            .worktree_directory(worktree.id)
                            .map(|path| SessionWorktree {
                                id: worktree.id,
                                name: worktree.name.clone(),
                                path,
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self
            .selected_worktree
            .is_some_and(|selected| !self.worktrees.iter().any(|it| it.id == selected))
        {
            self.selected_worktree = None;
        }
        self.rebuild_items(ctx);
    }

    fn scope_paths(&self) -> Vec<std::path::PathBuf> {
        let mut paths = self.workspace_paths.clone();
        paths.extend(self.worktrees.iter().map(|worktree| worktree.path.clone()));
        paths
    }

    fn selected_worktree(&self) -> Option<&SessionWorktree> {
        let selected = self.selected_worktree?;
        self.worktrees.iter().find(|it| it.id == selected)
    }

    fn worktree_name_for_session(&self, session: &AgentSession) -> Option<String> {
        let cwd = session.cwd.as_deref()?;
        self.worktrees
            .iter()
            .filter(|worktree| path_contains(&worktree.path, cwd))
            .max_by_key(|worktree| worktree.path.as_os_str().len())
            .map(|worktree| worktree.name.clone())
            .or_else(|| {
                cwd.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
    }

    fn rebuild_items(&mut self, ctx: &mut ViewContext<Self>) {
        let query = self.query_editor.as_ref(ctx).buffer_text(ctx);
        let filter = SessionFilter {
            query,
            enabled_agents: self.enabled_agents.clone(),
            sort: self.sort,
            workspace_paths: self.scope_paths(),
            worktree_path: self.selected_worktree().map(|it| it.path.clone()),
            hide_empty: self.hide_empty,
        };
        let sessions = filter_sessions(self.history.as_ref(ctx).sessions(), &filter);
        self.session_count = sessions.len();
        let groups = group_sessions(sessions, self.group);
        let mut items = Vec::new();
        for group in groups {
            let collapsed = self.collapsed_groups.contains(&group.key);
            items.push(ListItem::Group(group.clone()));
            if !collapsed {
                items.extend(
                    group
                        .sessions
                        .into_iter()
                        .map(Box::new)
                        .map(ListItem::Session),
                );
            }
        }
        self.action_mouse_states
            .retain(|id, _| self.expanded_sessions.contains(id));
        for id in &self.expanded_sessions {
            self.action_mouse_states
                .entry(id.clone())
                .or_insert_with(|| {
                    (0..MAX_SESSION_ACTIONS)
                        .map(|_| MouseStateHandle::default())
                        .collect()
                });
        }
        let old_len = self.items.len();
        self.row_mouse_states
            .resize_with(items.len(), MouseStateHandle::default);
        self.items = Arc::new(items);
        for _ in old_len..self.items.len() {
            self.list_state.add_item();
        }
        for index in (self.items.len()..old_len).rev() {
            self.list_state.remove(index);
        }
        for index in 0..self.items.len().min(old_len) {
            self.list_state.invalidate_height_for_index(index);
        }
        ctx.notify();
    }

    fn selectable_menu_item(
        label: impl Into<String>,
        selected: bool,
        action: SessionsAction,
    ) -> MenuItemFields<SessionsAction> {
        let fields = MenuItemFields::new(label.into()).with_on_select_action(action);
        if selected {
            fields.with_icon(Icon::Check)
        } else {
            fields.with_indent()
        }
    }

    fn worktree_menu_items(&self) -> Vec<MenuItem<SessionsAction>> {
        let mut items = vec![MenuItem::Item(Self::selectable_menu_item(
            "All worktrees",
            self.selected_worktree.is_none(),
            SessionsAction::SetWorktree(None),
        ))];
        items.extend(self.worktrees.iter().map(|worktree| {
            MenuItem::Item(Self::selectable_menu_item(
                worktree.name.clone(),
                self.selected_worktree == Some(worktree.id),
                SessionsAction::SetWorktree(Some(worktree.id)),
            ))
        }));
        items
    }

    fn agent_menu_items(&self, ctx: &ViewContext<Self>) -> Vec<MenuItem<SessionsAction>> {
        let scanned_agents = self
            .history
            .as_ref(ctx)
            .sessions()
            .iter()
            .map(|session| session.agent)
            .collect::<HashSet<_>>();
        let mut listed = SUPPORTED_AGENTS
            .into_iter()
            .filter(|agent| scanned_agents.contains(agent) || self.enabled_agents.contains(agent))
            .collect::<Vec<_>>();
        if listed.is_empty() {
            listed = SUPPORTED_AGENTS.to_vec();
        }
        let mut items = vec![
            MenuItem::Item(Self::selectable_menu_item(
                "All agents",
                self.enabled_agents.len() == SUPPORTED_AGENTS.len(),
                SessionsAction::EnableAllAgents,
            )),
            MenuItem::Separator,
        ];
        items.extend(listed.into_iter().map(|agent| {
            MenuItem::Item(Self::selectable_menu_item(
                agent.display_name(),
                self.enabled_agents.contains(&agent),
                SessionsAction::ToggleAgent(agent),
            ))
        }));
        items
    }

    fn options_menu_items(&self) -> Vec<MenuItem<SessionsAction>> {
        vec![
            MenuItem::Header {
                fields: MenuItemFields::new("Sort by"),
                clickable: false,
                right_side_fields: None,
            },
            MenuItem::Item(Self::selectable_menu_item(
                "Last updated",
                self.sort == SessionSort::Updated,
                SessionsAction::SetSort(SessionSort::Updated),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "Date created",
                self.sort == SessionSort::Created,
                SessionsAction::SetSort(SessionSort::Created),
            )),
            MenuItem::Separator,
            MenuItem::Header {
                fields: MenuItemFields::new("Group by"),
                clickable: false,
                right_side_fields: None,
            },
            MenuItem::Item(Self::selectable_menu_item(
                "Project",
                self.group == SessionGroup::Project,
                SessionsAction::SetGroup(SessionGroup::Project),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "Folder",
                self.group == SessionGroup::Folder,
                SessionsAction::SetGroup(SessionGroup::Folder),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "Agent",
                self.group == SessionGroup::Agent,
                SessionsAction::SetGroup(SessionGroup::Agent),
            )),
            MenuItem::Separator,
            MenuItem::Item(Self::selectable_menu_item(
                "Show empty sessions",
                !self.hide_empty,
                SessionsAction::ToggleHideEmpty,
            )),
            MenuItem::Separator,
            MenuItem::Header {
                fields: MenuItemFields::new("Sessions scanned"),
                clickable: false,
                right_side_fields: None,
            },
            MenuItem::Item(Self::selectable_menu_item(
                "250",
                self.limit == SessionLimit::TwoHundredFifty,
                SessionsAction::SetLimit(SessionLimit::TwoHundredFifty),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "500",
                self.limit == SessionLimit::FiveHundred,
                SessionsAction::SetLimit(SessionLimit::FiveHundred),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "1000",
                self.limit == SessionLimit::OneThousand,
                SessionsAction::SetLimit(SessionLimit::OneThousand),
            )),
            MenuItem::Item(Self::selectable_menu_item(
                "No limit",
                self.limit == SessionLimit::Unlimited,
                SessionsAction::SetLimit(SessionLimit::Unlimited),
            )),
            MenuItem::Separator,
            MenuItem::Item(
                MenuItemFields::new("Rescan sessions")
                    .with_icon(Icon::Refresh)
                    .with_on_select_action(SessionsAction::Refresh),
            ),
        ]
    }

    fn populate_filter_menu(&mut self, menu: FilterMenu, ctx: &mut ViewContext<Self>) {
        let items = match menu {
            FilterMenu::Worktree => self.worktree_menu_items(),
            FilterMenu::Agents => self.agent_menu_items(ctx),
            FilterMenu::Options => self.options_menu_items(),
        };
        self.filter_menu
            .update(ctx, |filter_menu, ctx| filter_menu.set_items(items, ctx));
    }

    fn worktree_label(&self) -> String {
        self.selected_worktree()
            .map(|worktree| worktree.name.clone())
            .unwrap_or_else(|| "All worktrees".to_owned())
    }

    fn agents_label(&self) -> String {
        match self.enabled_agents.len() {
            0 => "No agents".to_owned(),
            1 => self
                .enabled_agents
                .iter()
                .next()
                .map(|agent| agent.display_name().to_owned())
                .unwrap_or_default(),
            count if count == SUPPORTED_AGENTS.len() => "All agents".to_owned(),
            count => format!("{count} agents"),
        }
    }

    fn render_filter_button(
        label: String,
        anchor_id: &'static str,
        menu: FilterMenu,
        max_width: f32,
        mouse_state: MouseStateHandle,
        expanded: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let font_size = appearance.ui_font_size() - 1.;
        let mut button = appearance
            .ui_builder()
            .button(ButtonVariant::Outlined, mouse_state)
            .with_text_and_icon_label(
                TextAndIcon::new(
                    TextAndIconAlignment::TextFirst,
                    label,
                    Icon::ChevronDown.to_warpui_icon(theme.sub_text_color(theme.background())),
                    MainAxisSize::Min,
                    MainAxisAlignment::Start,
                    vec2f(12., 12.),
                )
                .with_inner_padding(4.),
            )
            .with_style(UiComponentStyles {
                font_size: Some(font_size),
                padding: Some(Coords {
                    top: 4.,
                    bottom: 4.,
                    left: 8.,
                    right: 6.,
                }),
                ..Default::default()
            })
            .with_active_styles(UiComponentStyles {
                background: Some(theme.surface_2().into()),
                ..Default::default()
            });
        if expanded {
            button = button.active();
        }
        SavePosition::new(
            ConstrainedBox::new(
                button
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(SessionsAction::ToggleFilterMenu(menu));
                    })
                    .finish(),
            )
            .with_max_width(max_width)
            .finish(),
            anchor_id,
        )
        .finish()
    }

    fn render_controls(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        if !self.worktrees.is_empty() {
            row.add_child(Self::render_filter_button(
                self.worktree_label(),
                WORKTREE_ANCHOR_ID,
                FilterMenu::Worktree,
                WORKTREE_BUTTON_MAX_WIDTH,
                self.worktree_button_mouse_state.clone(),
                self.open_filter_menu == Some(FilterMenu::Worktree),
                appearance,
            ));
        }
        row.with_child(Self::render_filter_button(
            self.agents_label(),
            AGENTS_ANCHOR_ID,
            FilterMenu::Agents,
            AGENTS_BUTTON_MAX_WIDTH,
            self.agents_button_mouse_state.clone(),
            self.open_filter_menu == Some(FilterMenu::Agents),
            appearance,
        ))
        .with_child(Expanded::new(1., Empty::new().finish()).finish())
        .with_child(
            SavePosition::new(
                icon_button(
                    appearance,
                    Icon::Sliders,
                    self.open_filter_menu == Some(FilterMenu::Options),
                    self.options_button_mouse_state.clone(),
                )
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(SessionsAction::ToggleFilterMenu(
                        FilterMenu::Options,
                    ));
                })
                .finish(),
                OPTIONS_ANCHOR_ID,
            )
            .finish(),
        )
        .finish()
    }

    fn render_search_field(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(
                    ConstrainedBox::new(
                        Icon::Search
                            .to_warpui_icon(theme.sub_text_color(theme.background()))
                            .finish(),
                    )
                    .with_width(14.)
                    .with_height(14.)
                    .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Clipped::new(ChildView::new(&self.query_editor).finish()).finish(),
                    )
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(8.)
        .with_vertical_padding(5.)
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish()
    }

    fn render_row_action(
        label: &'static str,
        action: SessionsAction,
        mouse_state: MouseStateHandle,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .button(ButtonVariant::Outlined, mouse_state)
            .with_text_label(label.to_owned())
            .with_style(UiComponentStyles {
                font_size: Some(appearance.ui_font_size() - 2.),
                padding: Some(Coords {
                    top: 3.,
                    bottom: 3.,
                    left: 8.,
                    right: 8.,
                }),
                ..Default::default()
            })
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish()
    }

    fn render_group(
        &self,
        group: &SessionGroupResult,
        index: usize,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let collapsed = self.collapsed_groups.contains(&group.key);
        let chevron = if collapsed {
            Icon::ChevronRight
        } else {
            Icon::ChevronDown
        };
        let label = Text::new_inline(
            group.label.clone(),
            appearance.ui_font_family(),
            appearance.ui_font_size() - 1.,
        )
        .with_style(Properties::default().weight(Weight::Semibold))
        .with_color(theme.main_text_color(theme.surface_1()).into())
        .with_clip(ClipConfig::ellipsis())
        .finish();
        let count = Text::new_inline(
            group.sessions.len().to_string(),
            appearance.ui_font_family(),
            appearance.ui_font_size() - 2.,
        )
        .with_color(theme.sub_text_color(theme.surface_1()).into())
        .finish();
        let key = group.key.clone();
        Hoverable::new(self.row_mouse_states[index].clone(), move |hovered| {
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.)
                    .with_child(
                        ConstrainedBox::new(
                            chevron
                                .to_warpui_icon(theme.sub_text_color(theme.surface_1()))
                                .finish(),
                        )
                        .with_width(12.)
                        .with_height(12.)
                        .finish(),
                    )
                    .with_child(Shrinkable::new(1., label).finish())
                    .with_child(count)
                    .finish(),
            )
            .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
            .with_vertical_padding(6.)
            .with_background(if hovered.is_hovered() {
                theme.surface_2()
            } else {
                theme.surface_1()
            })
            .finish()
        })
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(SessionsAction::ToggleGroup(key.clone()))
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn render_session(
        &self,
        session: &AgentSession,
        index: usize,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let expanded = self.expanded_sessions.contains(&session.id);
        let row_background = if expanded {
            theme.surface_1()
        } else {
            theme.background()
        };
        let live = CLIAgentSessionsModel::as_ref(app)
            .sessions()
            .find(|(_, live)| {
                live.agent == session.agent
                    && live.session_context.session_id.as_deref() == Some(&session.session_id)
            });
        let subtext = [
            Some(session.agent.display_name().to_owned()),
            self.worktree_name_for_session(session),
            Some(format_approx_duration_from_now_utc(
                session.effective_updated_at(),
            )),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let title = Text::new_inline(
            session.title.clone(),
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_color(theme.main_text_color(row_background).into())
        .with_clip(ClipConfig::ellipsis())
        .finish();
        let mut title_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(6.)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        if let Some(icon) = session.agent.icon() {
            title_row.add_child(
                ConstrainedBox::new(
                    icon.to_warpui_icon(theme.sub_text_color(row_background))
                        .finish(),
                )
                .with_width(16.)
                .with_height(16.)
                .finish(),
            );
        }
        title_row.add_child(Shrinkable::new(1., title).finish());
        let subtext = Text::new_inline(
            subtext,
            appearance.ui_font_family(),
            appearance.ui_font_size() - 2.,
        )
        .with_color(theme.sub_text_color(row_background).into())
        .with_clip(ClipConfig::ellipsis())
        .finish();
        let mut body = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(3.)
            .with_child(title_row.finish())
            .with_child(subtext);
        if expanded {
            for preview in &session.preview_messages {
                let prefix = match preview.role {
                    PreviewRole::User => "You",
                    PreviewRole::Assistant => "Agent",
                    PreviewRole::System => "System",
                    PreviewRole::Tool => "Tool",
                    PreviewRole::Unknown => "Message",
                };
                body.add_child(
                    Text::new_inline(
                        format!("{prefix}: {}", preview.text),
                        appearance.ui_font_family(),
                        appearance.ui_font_size() - 2.,
                    )
                    .with_color(theme.sub_text_color(row_background).into())
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
                );
            }
            let mut labels_and_actions: Vec<(&'static str, SessionsAction)> = Vec::new();
            if let Some((terminal_view_id, _)) = live {
                labels_and_actions.push((
                    "Jump to pane",
                    SessionsAction::JumpOriginal(terminal_view_id),
                ));
            }
            if session.has_resumable_content() {
                labels_and_actions.push(("Resume", SessionsAction::Resume(session.id.clone())));
            }
            if session.agent != CLIAgent::OpenCode {
                labels_and_actions.push(("Open log", SessionsAction::OpenLog(session.id.clone())));
            }
            labels_and_actions.push((
                "Copy command",
                SessionsAction::CopyCommand(session.id.clone()),
            ));
            if session.first_user_prompt.is_some() {
                labels_and_actions.push((
                    "Copy first prompt",
                    SessionsAction::CopyPrompt(session.id.clone()),
                ));
            }
            labels_and_actions.push(("Copy ID", SessionsAction::CopyId(session.id.clone())));
            labels_and_actions.push(("Copy path", SessionsAction::CopyPath(session.id.clone())));
            labels_and_actions.push(("Reveal", SessionsAction::RevealLog(session.id.clone())));
            if !matches!(
                session.agent,
                CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Antigravity
            ) {
                labels_and_actions.push(("Delete…", SessionsAction::Delete(session.id.clone())));
            }
            let mouse_states = self.action_mouse_states.get(&session.id);
            let mut actions = Wrap::row().with_spacing(4.).with_run_spacing(4.);
            actions.extend(labels_and_actions.into_iter().enumerate().map(
                |(action_index, (label, action))| {
                    let mouse_state = mouse_states
                        .and_then(|states| states.get(action_index))
                        .cloned()
                        .unwrap_or_default();
                    Self::render_row_action(label, action, mouse_state, appearance)
                },
            ));
            body.add_child(
                Container::new(actions.finish())
                    .with_padding_top(3.)
                    .finish(),
            );
        }
        let id = session.id.clone();
        Hoverable::new(self.row_mouse_states[index].clone(), move |hovered| {
            Container::new(body.finish())
                .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
                .with_vertical_padding(8.)
                .with_background(if hovered.is_hovered() && !expanded {
                    theme.surface_1()
                } else {
                    row_background
                })
                .with_border(Border::bottom(1.).with_border_fill(theme.outline()))
                .finish()
        })
        .with_defer_events_to_children()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(SessionsAction::ToggleSession(id.clone()))
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }
}

impl Entity for SessionsView {
    type Event = LeftPanelEvent;
}

impl TypedActionView for SessionsView {
    type Action = SessionsAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        let should_persist = matches!(
            action,
            SessionsAction::SetSort(_)
                | SessionsAction::SetGroup(_)
                | SessionsAction::SetLimit(_)
                | SessionsAction::ToggleAgent(_)
                | SessionsAction::EnableAllAgents
                | SessionsAction::ToggleHideEmpty
        );
        match action {
            SessionsAction::ToggleFilterMenu(menu) => {
                if self.open_filter_menu == Some(*menu) {
                    self.open_filter_menu = None;
                } else {
                    self.open_filter_menu = Some(*menu);
                    self.populate_filter_menu(*menu, ctx);
                    ctx.focus(&self.filter_menu);
                }
            }
            SessionsAction::SetWorktree(worktree_id) => {
                self.selected_worktree = worktree_id
                    .filter(|selected| self.worktrees.iter().any(|it| it.id == *selected));
            }
            SessionsAction::SetSort(sort) => self.sort = *sort,
            SessionsAction::SetGroup(group) => self.group = *group,
            SessionsAction::SetLimit(limit) => {
                self.limit = *limit;
                self.history
                    .update(ctx, |history, ctx| history.set_limit(self.limit, ctx));
            }
            SessionsAction::ToggleAgent(agent) => {
                if !self.enabled_agents.remove(agent) {
                    self.enabled_agents.insert(*agent);
                }
            }
            SessionsAction::EnableAllAgents => {
                self.enabled_agents = SUPPORTED_AGENTS.into_iter().collect();
            }
            SessionsAction::ToggleGroup(key) => {
                if !self.collapsed_groups.remove(key) {
                    self.collapsed_groups.insert(key.clone());
                }
            }
            SessionsAction::ToggleSession(id) => {
                if !self.expanded_sessions.remove(id) {
                    self.expanded_sessions.insert(id.clone());
                }
            }
            SessionsAction::ToggleHideEmpty => self.hide_empty = !self.hide_empty,
            SessionsAction::Refresh => self
                .history
                .update(ctx, |history, ctx| history.refresh(true, ctx)),
            SessionsAction::Resume(id) => {
                if let Some(session) = self
                    .history
                    .as_ref(ctx)
                    .sessions()
                    .iter()
                    .find(|session| session.id == *id)
                    .cloned()
                {
                    ctx.emit(LeftPanelEvent::ResumeAgentSession(session));
                }
            }
            SessionsAction::CopyCommand(id)
            | SessionsAction::CopyPrompt(id)
            | SessionsAction::CopyId(id)
            | SessionsAction::CopyPath(id) => {
                if let Some(session) = self
                    .history
                    .as_ref(ctx)
                    .sessions()
                    .iter()
                    .find(|session| session.id == *id)
                {
                    let value = match action {
                        SessionsAction::CopyCommand(_) => session.resume_command.clone(),
                        SessionsAction::CopyPrompt(_) => {
                            session.first_user_prompt.clone().unwrap_or_default()
                        }
                        SessionsAction::CopyId(_) => session.session_id.clone(),
                        SessionsAction::CopyPath(_) => {
                            session.transcript_path.display().to_string()
                        }
                        _ => unreachable!(),
                    };
                    ctx.clipboard().write(ClipboardContent::plain_text(value));
                }
            }
            SessionsAction::OpenLog(id) => {
                let path = self
                    .history
                    .as_ref(ctx)
                    .sessions()
                    .iter()
                    .find(|session| session.id == *id)
                    .map(|session| session.transcript_path.clone());
                if let Some(path) = path {
                    ctx.open_file_path(&path);
                }
            }
            SessionsAction::RevealLog(id) => {
                let path = self
                    .history
                    .as_ref(ctx)
                    .sessions()
                    .iter()
                    .find(|session| session.id == *id)
                    .map(|session| session.transcript_path.clone());
                if let Some(path) = path {
                    ctx.open_file_path_in_explorer(&path);
                }
            }
            SessionsAction::JumpOriginal(terminal_view_id) => {
                ctx.emit(LeftPanelEvent::FocusAgentSession(*terminal_view_id));
            }
            SessionsAction::Delete(id) => {
                if let Some(session) = self
                    .history
                    .as_ref(ctx)
                    .sessions()
                    .iter()
                    .find(|session| session.id == *id)
                    .cloned()
                {
                    ctx.emit(LeftPanelEvent::DeleteAgentSession(session));
                }
            }
        }
        if should_persist {
            self.persist_view_options(ctx);
            if let Some(menu) = self.open_filter_menu {
                self.populate_filter_menu(menu, ctx);
            }
        }
        self.rebuild_items(ctx);
    }
}

impl View for SessionsView {
    fn ui_name() -> &'static str {
        "SessionsView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let content = if self.items.is_empty() {
            let label = if self.history.as_ref(app).state() == ScanState::Loading {
                "Scanning agent sessions…"
            } else {
                "No matching sessions"
            };
            Flex::column()
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(
                        label,
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .finish(),
                )
                .finish()
        } else {
            Scrollable::vertical(
                self.scroll_state.clone(),
                List::new(self.list_state.clone()).finish_scrollable(),
                ScrollbarWidth::Auto,
                theme.nonactive_ui_detail().into(),
                theme.active_ui_detail().into(),
                Fill::None,
            )
            .with_overlayed_scrollbar()
            .finish()
        };
        let issues = self.history.as_ref(app).issues();
        let issue = (!issues.is_empty()).then(|| {
            Container::new(
                Text::new_inline(
                    format!(
                        "{} session file{} could not be read",
                        issues.len(),
                        if issues.len() == 1 { "" } else { "s" }
                    ),
                    appearance.ui_font_family(),
                    appearance.ui_font_size() - 2.,
                )
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
            )
            .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
            .with_padding_bottom(6.)
            .finish()
        });
        let mut header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(
                Text::new_inline(
                    "Sessions",
                    appearance.ui_font_family(),
                    appearance.ui_font_size() + 2.,
                )
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_color(theme.main_text_color(theme.background()).into())
                .finish(),
            );
        if self.session_count > 0 {
            header.add_child(
                Text::new_inline(
                    self.session_count.to_string(),
                    appearance.ui_font_family(),
                    appearance.ui_font_size() - 1.,
                )
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
            );
        }
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                SavePosition::new(
                    Container::new(header.finish())
                        .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
                        .with_padding_top(10.)
                        .with_padding_bottom(8.)
                        .finish(),
                    SESSIONS_PANEL_HEADER_POSITION_ID,
                )
                .finish(),
            )
            .with_child(
                Container::new(self.render_search_field(appearance))
                    .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
                    .with_padding_bottom(8.)
                    .finish(),
            )
            .with_child(
                Container::new(self.render_controls(appearance))
                    .with_horizontal_padding(ROW_HORIZONTAL_PADDING)
                    .with_padding_bottom(8.)
                    .finish(),
            );
        if let Some(issue) = issue {
            column.add_child(issue);
        }
        column.add_child(Shrinkable::new(1., content).finish());

        let mut stack = Stack::new().with_child(column.finish());
        if let Some(open_menu) = self.open_filter_menu {
            let (element_anchor, child_anchor) = match open_menu {
                FilterMenu::Worktree | FilterMenu::Agents => {
                    (PositionedElementAnchor::BottomLeft, ChildAnchor::TopLeft)
                }
                FilterMenu::Options => {
                    (PositionedElementAnchor::BottomRight, ChildAnchor::TopRight)
                }
            };
            stack.add_positioned_overlay_child(
                ChildView::new(&self.filter_menu).finish(),
                OffsetPositioning::offset_from_save_position_element(
                    open_menu.anchor_id(),
                    vec2f(0., 4.),
                    PositionedElementOffsetBounds::WindowByPosition,
                    element_anchor,
                    child_anchor,
                ),
            );
        }
        stack.finish()
    }
}
