use pathfinder_color::ColorU;
use warp_core::context_flag::ContextFlag;
use warpui::assets::asset_cache::AssetSource;
use warpui::elements::{
    Align, CacheOption, ConstrainedBox, Container, CrossAxisAlignment, Element, Flex, Image,
    MainAxisAlignment, MouseStateHandle, ParentElement, Wrap,
};
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Entity, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use super::SettingsSection;
use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget,
};
use crate::appearance::Appearance;
use crate::autoupdate::{self, AutoupdateStage, AutoupdateState};
use crate::channel::ChannelState;
use crate::themes::theme::ColorScheme;
use crate::workspace::WorkspaceAction;

#[derive(Debug, Clone)]
pub enum AboutPageAction {
    Relaunch,
    DownloadUpdate,
    CheckForUpdate,
}

#[derive(Clone, Copy)]
pub enum AboutPageEvent {
    CheckForUpdate,
}

pub struct AboutPageView {
    page: PageType<Self>,
}

impl AboutPageView {
    pub fn new(ctx: &mut ViewContext<AboutPageView>) -> Self {
        ctx.observe(
            &AutoupdateState::handle(ctx),
            Self::handle_autoupdate_state_change,
        );

        AboutPageView {
            page: PageType::new_monolith(AboutPageWidget::default(), None, false),
        }
    }

    fn handle_autoupdate_state_change(
        &mut self,
        _: ModelHandle<AutoupdateState>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.notify();
    }
}

impl Entity for AboutPageView {
    type Event = AboutPageEvent;
}

impl TypedActionView for AboutPageView {
    type Action = AboutPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AboutPageAction::Relaunch => {
                autoupdate::initiate_relaunch_for_update(ctx);
            }
            AboutPageAction::DownloadUpdate => {
                autoupdate::manually_download_new_version(ctx);
            }
            AboutPageAction::CheckForUpdate => {
                ctx.emit(AboutPageEvent::CheckForUpdate);
                ctx.notify();
            }
        }
    }
}

impl View for AboutPageView {
    fn ui_name() -> &'static str {
        "AboutPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

#[derive(Default)]
struct AboutPageWidget {
    copy_version_button_mouse_state: MouseStateHandle,
    update_cta_link_mouse_state: MouseStateHandle,
}

struct UpdateStatus {
    text: &'static str,
    color: ColorU,
}

struct UpdateCallToAction {
    text: &'static str,
    action: AboutPageAction,
}

impl AboutPageWidget {
    fn update_state(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> (Option<UpdateStatus>, Option<UpdateCallToAction>) {
        if !ContextFlag::PromptForVersionUpdates.is_enabled() {
            return (None, None);
        }

        let faded: ColorU = appearance
            .theme()
            .active_ui_text_color()
            .with_opacity(60)
            .into();
        let ansi_red: ColorU = appearance.theme().terminal_colors().bright.red.into();

        match autoupdate::get_update_state(app) {
            AutoupdateStage::NoUpdateAvailable => (
                Some(UpdateStatus {
                    text: "Up to date",
                    color: faded,
                }),
                Some(UpdateCallToAction {
                    text: "Check for updates",
                    action: AboutPageAction::CheckForUpdate,
                }),
            ),
            AutoupdateStage::CheckingForUpdate => (
                Some(UpdateStatus {
                    text: "Checking for update…",
                    color: faded,
                }),
                None,
            ),
            AutoupdateStage::DownloadingUpdate => (
                Some(UpdateStatus {
                    text: "Downloading update…",
                    color: faded,
                }),
                None,
            ),
            AutoupdateStage::UpdateReady { .. } => (
                Some(UpdateStatus {
                    text: "Update available",
                    color: ansi_red,
                }),
                Some(UpdateCallToAction {
                    text: "Relaunch Warp",
                    action: AboutPageAction::Relaunch,
                }),
            ),
            AutoupdateStage::Updating { .. } => (
                Some(UpdateStatus {
                    text: "Updating…",
                    color: faded,
                }),
                None,
            ),
            AutoupdateStage::UpdatedPendingRestart { .. } => (
                Some(UpdateStatus {
                    text: "Installed update",
                    color: faded,
                }),
                Some(UpdateCallToAction {
                    text: "Relaunch Warp",
                    action: AboutPageAction::Relaunch,
                }),
            ),
            AutoupdateStage::UnableToUpdateToNewVersion { .. } => (
                Some(UpdateStatus {
                    text: "A new version of Warp is available but can't be installed",
                    color: ansi_red,
                }),
                Some(UpdateCallToAction {
                    text: "Update Warp manually",
                    action: AboutPageAction::DownloadUpdate,
                }),
            ),
            AutoupdateStage::UnableToLaunchNewVersion { .. } => (
                Some(UpdateStatus {
                    text: "A new version of Warp is installed but can't be launched.",
                    color: ansi_red,
                }),
                Some(UpdateCallToAction {
                    text: "Update Warp manually",
                    action: AboutPageAction::DownloadUpdate,
                }),
            ),
        }
    }
}

impl SettingsWidget for AboutPageWidget {
    type View = AboutPageView;

    fn search_terms(&self) -> &str {
        "about warp version update"
    }

    fn render(
        &self,
        _view: &AboutPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder();

        let image_path = if theme.inferred_color_scheme() == ColorScheme::LightOnDark {
            "bundled/svg/warp-logo-with-light-title.svg"
        } else {
            "bundled/svg/warp-logo-with-dark-title.svg"
        };

        let version = ChannelState::app_version().unwrap_or("v#.##.###");

        let version_text = ui_builder
            .span(version.to_string())
            .with_soft_wrap()
            .build()
            .with_margin_top(16.)
            .finish();

        let copy_version_icon = appearance
            .ui_builder()
            .copy_button(16., self.copy_version_button_mouse_state.clone())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::CopyVersion(version));
            })
            .finish();

        let version_row = Wrap::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_children([
                version_text,
                Container::new(copy_version_icon)
                    .with_margin_top(16.)
                    .with_padding_left(6.)
                    .finish(),
            ]);

        let (status, call_to_action) = self.update_state(appearance, app);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                ConstrainedBox::new(
                    Image::new(
                        AssetSource::Bundled { path: image_path },
                        CacheOption::BySize,
                    )
                    .finish(),
                )
                .with_max_height(100.)
                .with_max_width(350.)
                .finish(),
            )
            .with_child(version_row.finish());

        if let Some(status) = status {
            column.add_child(
                ui_builder
                    .span(status.text.to_string())
                    .with_style(warpui::ui_components::components::UiComponentStyles {
                        font_color: Some(status.color),
                        ..Default::default()
                    })
                    .build()
                    .with_margin_top(8.)
                    .finish(),
            );
        }

        if let Some(call_to_action) = call_to_action {
            column.add_child(
                Container::new(
                    ui_builder
                        .link(
                            call_to_action.text.into(),
                            None,
                            Some(Box::new(move |ctx| {
                                ctx.dispatch_typed_action(call_to_action.action.clone());
                            })),
                            self.update_cta_link_mouse_state.clone(),
                        )
                        .soft_wrap(false)
                        .build()
                        .finish(),
                )
                .with_margin_top(4.)
                .finish(),
            );
        }

        column.add_child(
            ui_builder
                .span("Copyright 2026 Warp")
                .build()
                .with_margin_top(16.)
                .finish(),
        );

        Align::new(column.finish()).finish()
    }
}

impl SettingsPageMeta for AboutPageView {
    fn section() -> SettingsSection {
        SettingsSection::About
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<AboutPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AboutPageView>) -> Self {
        SettingsPageViewHandle::About(view_handle)
    }
}
