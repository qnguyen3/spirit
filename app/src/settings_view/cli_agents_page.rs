//! The "Third party CLI agents" settings page, shown under the Agents umbrella.

use markdown_parser::{FormattedText, FormattedTextFragment, FormattedTextLine};
use settings::Setting as _;
use warp_errors::report_if_error;
use warpui::elements::{Element, FormattedTextElement, HighlightedHyperlink};
use warpui::keymap::{ContextPredicate, FixedBinding};
use warpui::{
    Action, AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::settings_page::{
    MatchData, PageTitle, PageType, SettingsPageEvent, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, render_dropdown_item,
};
use super::{SettingsAction, SettingsSection};
use crate::appearance::Appearance;
use crate::settings::{AgentApprovalMode, CLIAgentSettings};
use crate::ui_components::blended_colors;
use crate::util::bindings;
use crate::view_components::{Dropdown, DropdownItem};

const PAGE_TITLE: &str = "Third party CLI agents";

pub struct CLIAgentsPageView {
    page: PageType<Self>,
    agent_approval_mode_dropdown: ViewHandle<Dropdown<CLIAgentsPageAction>>,
}

impl CLIAgentsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let agent_approval_mode_dropdown = ctx.add_typed_action_view(Dropdown::new);
        Self::update_agent_approval_mode_dropdown(agent_approval_mode_dropdown.clone(), ctx);

        ctx.subscribe_to_model(&CLIAgentSettings::handle(ctx), |me, _, _, ctx| {
            Self::update_agent_approval_mode_dropdown(me.agent_approval_mode_dropdown.clone(), ctx);
            ctx.notify();
        });

        Self {
            page: Self::build_page(),
            agent_approval_mode_dropdown,
        }
    }

    fn build_page() -> PageType<Self> {
        let widgets: Vec<Box<dyn SettingsWidget<View = Self>>> =
            vec![Box::new(CLIAgentWidget), Box::new(AgentApprovalModeWidget)];
        PageType::new_uncategorized(widgets, Some(PageTitle::new(PAGE_TITLE)))
    }

    fn update_agent_approval_mode_dropdown(
        dropdown: ViewHandle<Dropdown<CLIAgentsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        dropdown.update(ctx, |dropdown, ctx| {
            let values = [AgentApprovalMode::Yolo, AgentApprovalMode::Normal];

            let current_value = *CLIAgentSettings::as_ref(ctx).agent_approval_mode.value();

            let selected_index = values
                .iter()
                .position(|val| *val == current_value)
                .unwrap_or(0);

            dropdown.set_items(
                values
                    .into_iter()
                    .map(|val| {
                        DropdownItem::new(
                            val.dropdown_item_label(),
                            CLIAgentsPageAction::SetApprovalMode(val),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_index(selected_index, ctx);
        });
    }
}

impl Entity for CLIAgentsPageView {
    type Event = SettingsPageEvent;
}

impl View for CLIAgentsPageView {
    fn ui_name() -> &'static str {
        "CLIAgentsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CLIAgentsPageAction {
    SetApprovalMode(AgentApprovalMode),
}

impl TypedActionView for CLIAgentsPageView {
    type Action = CLIAgentsPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            CLIAgentsPageAction::SetApprovalMode(mode) => {
                CLIAgentSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.agent_approval_mode.set_value(*mode, ctx));
                    ctx.notify();
                });
            }
        }
    }
}

impl SettingsPageMeta for CLIAgentsPageView {
    fn section() -> SettingsSection {
        SettingsSection::ThirdPartyCLIAgents
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

impl From<ViewHandle<CLIAgentsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<CLIAgentsPageView>) -> Self {
        SettingsPageViewHandle::CLIAgents(view_handle)
    }
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    app.register_fixed_bindings(vec![
        FixedBinding::empty(
            "Agent Approval Mode: YOLO".to_string(),
            builder(SettingsAction::CLIAgents(
                CLIAgentsPageAction::SetApprovalMode(AgentApprovalMode::Yolo),
            )),
            context.to_owned(),
        )
        .with_group(bindings::BindingGroup::Settings.as_str()),
        FixedBinding::empty(
            "Agent Approval Mode: Normal".to_string(),
            builder(SettingsAction::CLIAgents(
                CLIAgentsPageAction::SetApprovalMode(AgentApprovalMode::Normal),
            )),
            context.to_owned(),
        )
        .with_group(bindings::BindingGroup::Settings.as_str()),
    ]);
}

/// Widget id backing the `cli_agents` deeplink slug. Lives here alongside the
/// widget itself because the default `widget_id()` is the type's full path,
/// which changes whenever the widget moves modules.
#[cfg(not(target_family = "wasm"))]
pub fn cli_agent_settings_widget_id() -> &'static str {
    CLIAgentWidget::static_widget_id()
}

struct CLIAgentWidget;

impl SettingsWidget for CLIAgentWidget {
    type View = CLIAgentsPageView;

    fn search_terms(&self) -> &str {
        "third party cli coding agent claude codex gemini toolbar footer quick actions"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let description_fragments = vec![
            FormattedTextFragment::plain_text(
                "Warp shows a toolbar with quick actions when running coding agents like ",
            ),
            FormattedTextFragment::inline_code("claude"),
            FormattedTextFragment::plain_text(", "),
            FormattedTextFragment::inline_code("codex"),
            FormattedTextFragment::plain_text(", or "),
            FormattedTextFragment::inline_code("gemini"),
            FormattedTextFragment::plain_text("."),
        ];

        FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(description_fragments)]),
            appearance.ui_font_size(),
            appearance.ui_font_family(),
            appearance.monospace_font_family(),
            blended_colors::text_sub(appearance.theme(), appearance.theme().surface_1()),
            HighlightedHyperlink::default(),
        )
        .finish()
    }
}

struct AgentApprovalModeWidget;

impl SettingsWidget for AgentApprovalModeWidget {
    type View = CLIAgentsPageView;

    fn search_terms(&self) -> &str {
        "third party cli coding agent approval mode yolo normal prompts permissions bypass launch"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        render_dropdown_item(
            appearance,
            "Agent Approval Mode:",
            Some(
                "YOLO launches agents with their approval prompts bypassed. Normal leaves each agent's own approval prompts in place.",
            ),
            None,
            None,
            &view.agent_approval_mode_dropdown,
        )
    }
}

#[cfg(test)]
#[path = "cli_agents_page_tests.rs"]
mod tests;
