use std::path::{Path, PathBuf};

use pathfinder_color::ColorU;
use settings::Setting as _;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Flex,
    Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    Text,
};
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::git_ops::CloneProgress;
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions};

const SECTION_GAP: f32 = 14.;
const CONTENT_PADDING: f32 = 24.;
const TITLE_FONT_SIZE: f32 = 16.;
const LABEL_FONT_SIZE: f32 = 12.;
const BUTTON_HEIGHT: f32 = 32.;
const BUTTON_RADIUS: Radius = Radius::Pixels(4.);
const MODE_TAB_RADIUS: Radius = Radius::Pixels(4.);
const ERROR_TEXT_COLOR: u32 = 0xBC362AFF;

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;
    app.register_fixed_bindings(vec![FixedBinding::new(
        "escape",
        NewWorkspaceModalAction::Escape,
        id!("NewWorkspaceModal"),
    )]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWorkspaceMode {
    Open,
    Clone,
    Create,
}

impl NewWorkspaceMode {
    fn label(self) -> &'static str {
        match self {
            NewWorkspaceMode::Open => "Open",
            NewWorkspaceMode::Clone => "Clone",
            NewWorkspaceMode::Create => "Create",
        }
    }

    fn all() -> [NewWorkspaceMode; 3] {
        [
            NewWorkspaceMode::Open,
            NewWorkspaceMode::Clone,
            NewWorkspaceMode::Create,
        ]
    }
}

#[derive(Debug, Clone)]
pub enum NewWorkspaceModalEvent {
    Close,
    BrowseFolder(NewWorkspaceMode),
    OpenFolder {
        path: PathBuf,
    },
    CloneRepo {
        url: String,
        parent: PathBuf,
        directory_name: String,
    },
    CreateProject {
        name: String,
        parent: PathBuf,
    },
    CancelClone,
}

#[derive(Debug, Clone, Copy)]
pub enum NewWorkspaceModalAction {
    SetMode(NewWorkspaceMode),
    Browse,
    Submit,
    Cancel,
    Escape,
}

pub struct NewWorkspaceModal {
    mode: NewWorkspaceMode,
    open_path_editor: ViewHandle<EditorView>,
    clone_url_editor: ViewHandle<EditorView>,
    clone_name_editor: ViewHandle<EditorView>,
    create_name_editor: ViewHandle<EditorView>,
    clone_parent: PathBuf,
    create_parent: PathBuf,
    in_flight: bool,
    clone_progress: Option<CloneProgress>,
    error: Option<String>,
    mode_mouse_states: Vec<MouseStateHandle>,
    browse_mouse_state: MouseStateHandle,
    submit_mouse_state: MouseStateHandle,
    cancel_mouse_state: MouseStateHandle,
}

impl Entity for NewWorkspaceModal {
    type Event = NewWorkspaceModalEvent;
}

impl TypedActionView for NewWorkspaceModal {
    type Action = NewWorkspaceModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            NewWorkspaceModalAction::SetMode(mode) => self.set_mode(*mode, ctx),
            NewWorkspaceModalAction::Browse => {
                ctx.emit(NewWorkspaceModalEvent::BrowseFolder(self.mode));
            }
            NewWorkspaceModalAction::Submit => self.try_submit(ctx),
            NewWorkspaceModalAction::Cancel | NewWorkspaceModalAction::Escape => {
                if self.in_flight && self.mode == NewWorkspaceMode::Clone {
                    ctx.emit(NewWorkspaceModalEvent::CancelClone);
                } else {
                    ctx.emit(NewWorkspaceModalEvent::Close);
                }
            }
        }
    }
}

impl NewWorkspaceModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let open_path_editor = Self::build_editor("/path/to/repo", ctx);
        let clone_url_editor = Self::build_editor("https://github.com/owner/repo.git", ctx);
        let clone_name_editor = Self::build_editor("repo", ctx);
        let create_name_editor = Self::build_editor("my-project", ctx);

        Self {
            mode: NewWorkspaceMode::Open,
            open_path_editor,
            clone_url_editor,
            clone_name_editor,
            create_name_editor,
            clone_parent: default_clone_parent(),
            create_parent: default_create_parent(),
            in_flight: false,
            clone_progress: None,
            error: None,
            mode_mouse_states: vec![
                MouseStateHandle::default(),
                MouseStateHandle::default(),
                MouseStateHandle::default(),
            ],
            browse_mouse_state: MouseStateHandle::default(),
            submit_mouse_state: MouseStateHandle::default(),
            cancel_mouse_state: MouseStateHandle::default(),
        }
    }

    fn build_editor(placeholder: &str, ctx: &mut ViewContext<Self>) -> ViewHandle<EditorView> {
        let placeholder = placeholder.to_owned();
        let editor = ctx.add_typed_action_view(move |ctx| {
            let mut editor = EditorView::single_line(SingleLineEditorOptions::default(), ctx);
            editor.set_placeholder_text(&placeholder, ctx);
            editor
        });
        ctx.subscribe_to_view(&editor, |me, _, event, ctx| match event {
            EditorEvent::Enter => me.try_submit(ctx),
            EditorEvent::Escape => ctx.emit(NewWorkspaceModalEvent::Close),
            EditorEvent::Edited(_) => {
                me.error = None;
                if me.mode == NewWorkspaceMode::Clone {
                    me.sync_clone_name_from_url(ctx);
                }
                ctx.notify();
            }
            _ => {}
        });
        editor
    }

    pub fn on_open(&mut self, mode: Option<NewWorkspaceMode>, ctx: &mut ViewContext<Self>) {
        self.mode = mode.unwrap_or(NewWorkspaceMode::Open);
        self.in_flight = false;
        self.clone_progress = None;
        self.error = None;
        let creation_settings = crate::projects::settings::WorkspaceCreationSettings::as_ref(ctx);
        self.clone_parent = remembered_parent(creation_settings.last_clone_parent.value())
            .unwrap_or_else(default_clone_parent);
        self.create_parent = remembered_parent(creation_settings.last_create_parent.value())
            .unwrap_or_else(default_create_parent);
        for editor in [
            self.open_path_editor.clone(),
            self.clone_url_editor.clone(),
            self.clone_name_editor.clone(),
            self.create_name_editor.clone(),
        ] {
            editor.update(ctx, |editor, ctx| {
                editor.clear_buffer_and_reset_undo_stack(ctx);
            });
        }
        ctx.focus(&self.active_editor());
        ctx.notify();
    }

    pub fn set_error(&mut self, error: String, ctx: &mut ViewContext<Self>) {
        self.error = Some(error);
        self.in_flight = false;
        self.clone_progress = None;
        ctx.notify();
    }

    pub fn set_clone_progress(&mut self, progress: CloneProgress, ctx: &mut ViewContext<Self>) {
        self.clone_progress = Some(progress);
        ctx.notify();
    }

    pub fn set_selected_folder(&mut self, path: PathBuf, ctx: &mut ViewContext<Self>) {
        match self.mode {
            NewWorkspaceMode::Open => {
                let text = path.to_string_lossy().to_string();
                self.open_path_editor.update(ctx, |editor, ctx| {
                    editor.set_buffer_text(&text, ctx);
                });
            }
            NewWorkspaceMode::Clone => self.clone_parent = path,
            NewWorkspaceMode::Create => self.create_parent = path,
        }
        self.error = None;
        ctx.notify();
    }

    fn active_editor(&self) -> ViewHandle<EditorView> {
        match self.mode {
            NewWorkspaceMode::Open => self.open_path_editor.clone(),
            NewWorkspaceMode::Clone => self.clone_url_editor.clone(),
            NewWorkspaceMode::Create => self.create_name_editor.clone(),
        }
    }

    fn set_mode(&mut self, mode: NewWorkspaceMode, ctx: &mut ViewContext<Self>) {
        if self.in_flight || self.mode == mode {
            return;
        }
        self.mode = mode;
        self.error = None;
        ctx.focus(&self.active_editor());
        ctx.notify();
    }

    fn sync_clone_name_from_url(&mut self, ctx: &mut ViewContext<Self>) {
        let url = self.clone_url_editor.as_ref(ctx).buffer_text(ctx);
        let current_name = self.clone_name_editor.as_ref(ctx).buffer_text(ctx);
        if !current_name.trim().is_empty() {
            return;
        }
        if let Some(derived) = super::git_ops::derive_clone_directory_name(&url) {
            self.clone_name_editor.update(ctx, |editor, ctx| {
                editor.set_buffer_text(&derived, ctx);
            });
        }
    }

    fn clone_target_name(&self, app: &AppContext) -> Option<String> {
        let typed = self.clone_name_editor.as_ref(app).buffer_text(app);
        let typed = typed.trim();
        if !typed.is_empty() {
            return Some(typed.to_owned());
        }
        let url = self.clone_url_editor.as_ref(app).buffer_text(app);
        super::git_ops::derive_clone_directory_name(&url)
    }

    fn try_submit(&mut self, ctx: &mut ViewContext<Self>) {
        if self.in_flight {
            return;
        }
        match self.mode {
            NewWorkspaceMode::Open => {
                let text = self.open_path_editor.as_ref(ctx).buffer_text(ctx);
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    self.error = Some("Choose a folder to open".to_owned());
                    ctx.notify();
                    return;
                }
                self.in_flight = true;
                ctx.notify();
                ctx.emit(NewWorkspaceModalEvent::OpenFolder {
                    path: expand_home(trimmed),
                });
            }
            NewWorkspaceMode::Clone => {
                let url = self.clone_url_editor.as_ref(ctx).buffer_text(ctx);
                let url = url.trim().to_owned();
                if url.is_empty() {
                    self.error = Some("Enter a repository URL".to_owned());
                    ctx.notify();
                    return;
                }
                let Some(directory_name) = self.clone_target_name(ctx) else {
                    self.error = Some("Enter a folder name for the clone".to_owned());
                    ctx.notify();
                    return;
                };
                self.in_flight = true;
                self.clone_progress = None;
                ctx.notify();
                ctx.emit(NewWorkspaceModalEvent::CloneRepo {
                    url,
                    parent: self.clone_parent.clone(),
                    directory_name,
                });
            }
            NewWorkspaceMode::Create => {
                let name = self.create_name_editor.as_ref(ctx).buffer_text(ctx);
                let name = name.trim().to_owned();
                if let Err(message) = validate_project_name(&name) {
                    self.error = Some(message);
                    ctx.notify();
                    return;
                }
                self.in_flight = true;
                ctx.notify();
                ctx.emit(NewWorkspaceModalEvent::CreateProject {
                    name,
                    parent: self.create_parent.clone(),
                });
            }
        }
    }

    fn render_label(text: &str, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Text::new_inline(
                text.to_owned(),
                appearance.ui_font_family(),
                LABEL_FONT_SIZE,
            )
            .with_color(theme.sub_text_color(theme.background()).into())
            .finish(),
        )
        .with_margin_bottom(4.)
        .finish()
    }

    fn render_mode_tabs(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.);
        for (index, mode) in NewWorkspaceMode::all().into_iter().enumerate() {
            let is_active = mode == self.mode;
            let mouse_state = self.mode_mouse_states[index].clone();
            let label = mode.label().to_owned();
            let font_family = appearance.ui_font_family();
            let font_size = appearance.ui_font_size();
            let tab = Hoverable::new(mouse_state, move |state| {
                let background = if is_active {
                    internal_colors::fg_overlay_2(theme)
                } else if state.is_hovered() {
                    internal_colors::fg_overlay_1(theme)
                } else {
                    theme.background()
                };
                Container::new(
                    Text::new_inline(label.clone(), font_family, font_size)
                        .with_color(theme.foreground().into())
                        .finish(),
                )
                .with_background(background)
                .with_corner_radius(CornerRadius::with_all(MODE_TAB_RADIUS))
                .with_horizontal_padding(12.)
                .with_vertical_padding(6.)
                .finish()
            })
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(NewWorkspaceModalAction::SetMode(mode));
            })
            .finish();
            row.add_child(tab);
        }
        row.finish()
    }

    fn render_button(
        label: String,
        mouse_state: MouseStateHandle,
        emphasized: bool,
        enabled: bool,
        action: NewWorkspaceModalAction,
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

    fn render_path_row(
        &self,
        label: &str,
        path: &Path,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Flex::column()
            .with_children([
                Self::render_label(label, appearance),
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_children([
                        Text::new_inline(
                            path.to_string_lossy().to_string(),
                            appearance.monospace_font_family(),
                            LABEL_FONT_SIZE,
                        )
                        .with_color(theme.foreground().into())
                        .finish(),
                        Self::render_button(
                            "Browse\u{2026}".to_owned(),
                            self.browse_mouse_state.clone(),
                            false,
                            !self.in_flight,
                            NewWorkspaceModalAction::Browse,
                            appearance,
                        ),
                    ])
                    .finish(),
            ])
            .finish()
    }

    fn render_status(&self, appearance: &Appearance) -> Option<Box<dyn Element>> {
        let theme = appearance.theme();
        if let Some(error) = &self.error {
            return Some(
                Text::new_inline(error.clone(), appearance.ui_font_family(), LABEL_FONT_SIZE)
                    .with_color(ColorU::from_u32(ERROR_TEXT_COLOR))
                    .finish(),
            );
        }
        let progress = self.clone_progress?;
        let text = match progress.percent {
            Some(percent) => format!("{} {percent}%", progress.phase.label()),
            None => progress.phase.label().to_owned(),
        };
        Some(
            Text::new_inline(text, appearance.ui_font_family(), LABEL_FONT_SIZE)
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
        )
    }
}

impl View for NewWorkspaceModal {
    fn ui_name() -> &'static str {
        "NewWorkspaceModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(SECTION_GAP)
            .with_child(
                Text::new_inline(
                    "New Workspace".to_owned(),
                    appearance.ui_font_family(),
                    TITLE_FONT_SIZE,
                )
                .with_color(theme.foreground().into())
                .finish(),
            )
            .with_child(self.render_mode_tabs(appearance));

        match self.mode {
            NewWorkspaceMode::Open => {
                body.add_child(
                    Flex::column()
                        .with_children([
                            Self::render_label("Folder", appearance),
                            ChildView::new(&self.open_path_editor).finish(),
                        ])
                        .finish(),
                );
                body.add_child(Self::render_button(
                    "Browse\u{2026}".to_owned(),
                    self.browse_mouse_state.clone(),
                    false,
                    !self.in_flight,
                    NewWorkspaceModalAction::Browse,
                    appearance,
                ));
            }
            NewWorkspaceMode::Clone => {
                body.add_child(
                    Flex::column()
                        .with_children([
                            Self::render_label("Repository URL", appearance),
                            ChildView::new(&self.clone_url_editor).finish(),
                        ])
                        .finish(),
                );
                body.add_child(
                    Flex::column()
                        .with_children([
                            Self::render_label("Folder name", appearance),
                            ChildView::new(&self.clone_name_editor).finish(),
                        ])
                        .finish(),
                );
                body.add_child(self.render_path_row("Clone into", &self.clone_parent, appearance));
                if let Some(name) = self.clone_target_name(app) {
                    body.add_child(
                        Text::new_inline(
                            self.clone_parent.join(name).to_string_lossy().to_string(),
                            appearance.monospace_font_family(),
                            LABEL_FONT_SIZE,
                        )
                        .with_color(theme.sub_text_color(theme.background()).into())
                        .finish(),
                    );
                }
            }
            NewWorkspaceMode::Create => {
                body.add_child(
                    Flex::column()
                        .with_children([
                            Self::render_label("Name", appearance),
                            ChildView::new(&self.create_name_editor).finish(),
                        ])
                        .finish(),
                );
                body.add_child(self.render_path_row("Create in", &self.create_parent, appearance));
            }
        }

        if let Some(status) = self.render_status(appearance) {
            body.add_child(status);
        }

        let submit_label = match self.mode {
            NewWorkspaceMode::Open => "Open",
            NewWorkspaceMode::Clone => {
                if self.in_flight {
                    "Cloning\u{2026}"
                } else {
                    "Clone"
                }
            }
            NewWorkspaceMode::Create => "Create",
        };

        let footer = Flex::row()
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
                    NewWorkspaceModalAction::Cancel,
                    appearance,
                ),
                Self::render_button(
                    submit_label.to_owned(),
                    self.submit_mouse_state.clone(),
                    true,
                    !self.in_flight,
                    NewWorkspaceModalAction::Submit,
                    appearance,
                ),
            ])
            .finish();

        body.add_child(footer);

        Container::new(body.finish())
            .with_padding(Padding::uniform(CONTENT_PADDING))
            .finish()
    }
}

fn remembered_parent(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn default_clone_parent() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn default_create_parent() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join("spirit").join("projects"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    let rest = rest.trim_start_matches(['/', '\\']);
    match dirs::home_dir() {
        Some(home) if rest.is_empty() => home,
        Some(home) => home.join(rest),
        None => PathBuf::from(path),
    }
}

fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a name for the new Workspace".to_owned());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain a path separator".to_owned());
    }
    if name == "." || name == ".." {
        return Err("Name cannot be '.' or '..'".to_owned());
    }
    Ok(())
}

#[cfg(test)]
#[path = "new_workspace_modal_tests.rs"]
mod tests;
