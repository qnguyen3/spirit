use settings::Setting as _;
use warp_errors::report_if_error;
use warpui::elements::{ChildView, ConstrainedBox, Element, Flex, MouseStateHandle, ParentElement};
use warpui::keymap::{ContextPredicate, FixedBinding};
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    Action, AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle, id,
};

use super::settings_page::{
    MatchData, PageTitle, PageType, SettingsPageEvent, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, render_body_item,
};
use super::{
    SettingsAction, SettingsSection, ToggleSettingActionPair, ToggleState, flags,
    remote_control_page_is_available,
};
use crate::appearance::Appearance;
use crate::features::FeatureFlag;
#[cfg(not(target_family = "wasm"))]
use crate::remote_control::{RemoteControlServer, ServerState};
use crate::settings::{RemoteControlSecrets, RemoteControlSettings, clamp_port};
use crate::view_components::{DismissibleToast, SubmittableTextInput, SubmittableTextInputEvent};
use crate::workspace::{ToastStack, WorkspaceAction};

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    if !remote_control_page_is_available() {
        return;
    }

    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "Remote Control",
            builder(SettingsAction::RemoteControlPageToggle(
                RemoteControlPageAction::ToggleEnabled,
            )),
            context,
            flags::REMOTE_CONTROL_CONTEXT_FLAG,
        )],
        app,
    );

    app.register_fixed_bindings([
        FixedBinding::empty(
            "Copy Remote Control URL",
            WorkspaceAction::CopyRemoteControlUrl,
            id!("Workspace"),
        )
        .with_enabled(|| FeatureFlag::RemoteControl.is_enabled()),
        FixedBinding::empty(
            "Open Remote Control in Browser",
            WorkspaceAction::OpenRemoteControlInBrowser,
            id!("Workspace"),
        )
        .with_enabled(|| FeatureFlag::RemoteControl.is_enabled()),
    ]);
}

const MASKED_TOKEN: &str = "••••••••••••";
const PORT_INPUT_WIDTH: f32 = 140.;
const URL_ROW_MAX_WIDTH: f32 = 320.;
const UNAVAILABLE_URL: &str = "Not available while Remote Control is off";

#[derive(Clone, Debug, PartialEq)]
pub enum RemoteControlPageAction {
    ToggleEnabled,
    ToggleLanAccess,
    ToggleAccessTokenVisibility,
    BeginRotateAccessToken,
    ConfirmRotateAccessToken,
    CancelRotateAccessToken,
}

pub struct RemoteControlSettingsPageView {
    page: PageType<Self>,
    port_input: ViewHandle<SubmittableTextInput>,
    access_token_revealed: bool,
    rotate_confirmation_pending: bool,
}

impl RemoteControlSettingsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let port_input = ctx.add_typed_action_view(|ctx| {
            let placeholder = placeholder_port_text(ctx);
            let mut input = SubmittableTextInput::new(ctx)
                .validate_on_edit(|port| port.trim().parse::<u16>().is_ok());
            input.set_placeholder_text(placeholder, ctx);
            input
        });
        ctx.subscribe_to_view(&port_input, Self::handle_port_input_event);

        if FeatureFlag::RemoteControl.is_enabled() {
            ctx.subscribe_to_model(&RemoteControlSettings::handle(ctx), |view, _, _, ctx| {
                view.refresh_port_placeholder(ctx);
                ctx.notify();
            });
            ctx.subscribe_to_model(&RemoteControlSecrets::handle(ctx), |_, _, _, ctx| {
                ctx.notify();
            });
            #[cfg(not(target_family = "wasm"))]
            ctx.subscribe_to_model(&RemoteControlServer::handle(ctx), |_, _, _, ctx| {
                ctx.notify();
            });
        }

        let widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![
            Box::new(EnableRemoteControlWidget::default()),
            Box::new(ServerStatusWidget),
            Box::new(PairingUrlWidget::default()),
            Box::new(ServerPortWidget),
            Box::new(AccessTokenWidget::default()),
            Box::new(LanAccessWidget::default()),
        ];

        Self {
            page: PageType::new_uncategorized(widgets, Some(PageTitle::new("Remote Control"))),
            port_input,
            access_token_revealed: false,
            rotate_confirmation_pending: false,
        }
    }

    fn refresh_port_placeholder(&mut self, ctx: &mut ViewContext<Self>) {
        let placeholder = placeholder_port_text(ctx);
        self.port_input.update(ctx, |input, ctx| {
            input.set_placeholder_text(placeholder, ctx);
        });
    }

    fn handle_port_input_event(
        &mut self,
        _handle: ViewHandle<SubmittableTextInput>,
        event: &SubmittableTextInputEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            SubmittableTextInputEvent::Submit(port) => self.submit_port(port, ctx),
            SubmittableTextInputEvent::Escape => ctx.emit(SettingsPageEvent::FocusModal),
        }
    }

    fn submit_port(&mut self, port: &str, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::RemoteControl.is_enabled() {
            return;
        }
        let Ok(port) = port.trim().parse::<u16>() else {
            return;
        };
        let port = clamp_port(port);
        RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(settings.remote_control_port.set_value(port, ctx));
        });
        self.refresh_port_placeholder(ctx);
        ctx.notify();
    }

    fn toggle_enabled(&mut self, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::RemoteControl.is_enabled() {
            return;
        }
        let enabled = RemoteControlSettings::as_ref(ctx).is_enabled();
        RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(settings.remote_control_enabled.set_value(!enabled, ctx));
        });
        ctx.notify();
    }

    fn toggle_lan_access(&mut self, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::RemoteControl.is_enabled() {
            return;
        }
        let allowed = RemoteControlSettings::as_ref(ctx).allows_lan_access();
        RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
            report_if_error!(
                settings
                    .remote_control_allow_lan_access
                    .set_value(!allowed, ctx)
            );
        });
        ctx.notify();
    }

    fn rotate_access_token(&mut self, ctx: &mut ViewContext<Self>) {
        self.rotate_confirmation_pending = false;
        if !FeatureFlag::RemoteControl.is_enabled() {
            return;
        }
        self.access_token_revealed = false;
        #[cfg(not(target_family = "wasm"))]
        RemoteControlServer::rotate_access_token(ctx);
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::success(
                    "Rotated. Every paired device must pair again.".to_owned(),
                ),
                window_id,
                ctx,
            );
        });
        ctx.notify();
    }

    fn render_paired_devices_slot(&self) -> Option<Box<dyn Element>> {
        None
    }
}

impl Entity for RemoteControlSettingsPageView {
    type Event = SettingsPageEvent;
}

impl TypedActionView for RemoteControlSettingsPageView {
    type Action = RemoteControlPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            RemoteControlPageAction::ToggleEnabled => self.toggle_enabled(ctx),
            RemoteControlPageAction::ToggleLanAccess => self.toggle_lan_access(ctx),
            RemoteControlPageAction::ToggleAccessTokenVisibility => {
                self.access_token_revealed = !self.access_token_revealed;
                ctx.notify();
            }
            RemoteControlPageAction::BeginRotateAccessToken => {
                self.rotate_confirmation_pending = true;
                ctx.notify();
            }
            RemoteControlPageAction::ConfirmRotateAccessToken => self.rotate_access_token(ctx),
            RemoteControlPageAction::CancelRotateAccessToken => {
                self.rotate_confirmation_pending = false;
                ctx.notify();
            }
        }
    }
}

impl View for RemoteControlSettingsPageView {
    fn ui_name() -> &'static str {
        "RemoteControlSettingsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for RemoteControlSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::RemoteControl
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        remote_control_page_is_available()
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

impl From<ViewHandle<RemoteControlSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<RemoteControlSettingsPageView>) -> Self {
        SettingsPageViewHandle::RemoteControl(view_handle)
    }
}

fn placeholder_port_text(ctx: &AppContext) -> String {
    if !FeatureFlag::RemoteControl.is_enabled() {
        return String::new();
    }
    RemoteControlSettings::as_ref(ctx).port().to_string()
}

#[derive(Default)]
struct ServerSnapshot {
    running: bool,
    url: Option<String>,
    lan_urls: Vec<String>,
    connected_clients: usize,
    failure: Option<String>,
}

impl ServerSnapshot {
    fn read(app: &AppContext) -> Self {
        if !remote_control_page_is_available() {
            return Self::default();
        }
        #[cfg(not(target_family = "wasm"))]
        let snapshot = Self::from_server_state(RemoteControlServer::as_ref(app).state());
        #[cfg(target_family = "wasm")]
        let snapshot = {
            let _ = app;
            Self::default()
        };
        snapshot
    }

    #[cfg(not(target_family = "wasm"))]
    fn from_server_state(state: &ServerState) -> Self {
        match state {
            ServerState::Stopped => Self::default(),
            ServerState::Running {
                endpoint,
                lan_endpoints,
                connected_clients,
                ..
            } => Self {
                running: true,
                url: Some(endpoint.url()),
                lan_urls: lan_endpoints
                    .iter()
                    .map(|endpoint| endpoint.url())
                    .collect(),
                connected_clients: *connected_clients,
                failure: None,
            },
            ServerState::Failed { message } => Self {
                failure: Some(message.clone()),
                ..Self::default()
            },
        }
    }

    fn status_text(&self) -> String {
        if let Some(failure) = &self.failure {
            return format!("Failed: {failure}");
        }
        let Some(url) = &self.url else {
            return "Stopped".to_owned();
        };
        let connected_clients = self.connected_clients;
        let devices = if connected_clients == 1 {
            "1 device connected".to_owned()
        } else {
            format!("{connected_clients} devices connected")
        };
        format!("Running at {url} · {devices}")
    }

    fn masked_pairing_url(&self) -> String {
        match &self.url {
            Some(url) => format!("{url}/pair?t={MASKED_TOKEN}"),
            None => UNAVAILABLE_URL.to_owned(),
        }
    }
}

fn access_token_text(revealed: bool, app: &AppContext) -> String {
    if !remote_control_page_is_available() {
        return MASKED_TOKEN.to_owned();
    }
    let token = RemoteControlSecrets::as_ref(app).access_token();
    match (revealed, token) {
        (true, Some(token)) => token.reveal().to_owned(),
        (false, Some(_)) | (true, None) | (false, None) => MASKED_TOKEN.to_owned(),
    }
}

fn read_only_row(text: String, appearance: &Appearance) -> Box<dyn Element> {
    ConstrainedBox::new(
        appearance
            .ui_builder()
            .span(text)
            .with_soft_wrap()
            .build()
            .finish(),
    )
    .with_max_width(URL_ROW_MAX_WIDTH)
    .finish()
}

fn action_button(
    label: &str,
    mouse_state: &MouseStateHandle,
    enabled: bool,
    appearance: &Appearance,
    action: RemoteControlPageAction,
) -> Box<dyn Element> {
    let button = appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, mouse_state.clone())
        .with_text_label(label.to_owned());
    if !enabled {
        return button.disabled().build().finish();
    }
    button
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
}

fn workspace_action_button(
    label: &str,
    mouse_state: &MouseStateHandle,
    enabled: bool,
    appearance: &Appearance,
    action: WorkspaceAction,
) -> Box<dyn Element> {
    let button = appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, mouse_state.clone())
        .with_text_label(label.to_owned());
    if !enabled {
        return button.disabled().build().finish();
    }
    button
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
}

#[derive(Default)]
struct EnableRemoteControlWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for EnableRemoteControlWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control enable server browser phone tablet mirror"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let enabled = RemoteControlSettings::as_ref(app).is_enabled();
        render_body_item::<RemoteControlPageAction>(
            "Enable Remote Control".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(enabled)
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(RemoteControlPageAction::ToggleEnabled);
                })
                .finish(),
            Some(
                "Serve the Remote Control web app from this machine so a browser can mirror and drive it."
                    .to_owned(),
            ),
        )
    }
}

struct ServerStatusWidget;

impl SettingsWidget for ServerStatusWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control status running stopped failed connected devices"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        render_body_item::<RemoteControlPageAction>(
            "Status".into(),
            None,
            ToggleState::Enabled,
            appearance,
            read_only_row(ServerSnapshot::read(app).status_text(), appearance),
            None,
        )
    }
}

#[derive(Default)]
struct PairingUrlWidget {
    copy_button_state: MouseStateHandle,
    open_button_state: MouseStateHandle,
}

impl SettingsWidget for PairingUrlWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control pairing url link copy open browser pair"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let snapshot = ServerSnapshot::read(app);
        let running = snapshot.running;
        let controls = Flex::row()
            .with_spacing(8.)
            .with_child(read_only_row(snapshot.masked_pairing_url(), appearance))
            .with_child(workspace_action_button(
                "Copy URL",
                &self.copy_button_state,
                running,
                appearance,
                WorkspaceAction::CopyRemoteControlUrl,
            ))
            .with_child(workspace_action_button(
                "Open in browser",
                &self.open_button_state,
                running,
                appearance,
                WorkspaceAction::OpenRemoteControlInBrowser,
            ))
            .finish();

        render_body_item::<RemoteControlPageAction>(
            "Pairing URL".into(),
            None,
            ToggleState::Enabled,
            appearance,
            controls,
            Some(
                "Open this link once in a browser to pair it. It carries the access token, so treat it like a password."
                    .to_owned(),
            ),
        )
    }
}

struct ServerPortWidget;

impl SettingsWidget for ServerPortWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control port listen tcp address"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        render_body_item::<RemoteControlPageAction>(
            "Port".into(),
            None,
            ToggleState::Enabled,
            appearance,
            ConstrainedBox::new(ChildView::new(&view.port_input).finish())
                .with_max_width(PORT_INPUT_WIDTH)
                .finish(),
            Some(
                "Restarts the server on the new port. Ports below 1024 are raised to 1024."
                    .to_owned(),
            ),
        )
    }
}

#[derive(Default)]
struct AccessTokenWidget {
    reveal_button_state: MouseStateHandle,
    copy_button_state: MouseStateHandle,
    rotate_button_state: MouseStateHandle,
    cancel_rotate_button_state: MouseStateHandle,
}

impl SettingsWidget for AccessTokenWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control access token secret reveal hide copy rotate pairing"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let reveal_label = if view.access_token_revealed {
            "Hide"
        } else {
            "Reveal"
        };
        let mut controls = Flex::row()
            .with_spacing(8.)
            .with_child(read_only_row(
                access_token_text(view.access_token_revealed, app),
                appearance,
            ))
            .with_child(action_button(
                reveal_label,
                &self.reveal_button_state,
                true,
                appearance,
                RemoteControlPageAction::ToggleAccessTokenVisibility,
            ))
            .with_child(workspace_action_button(
                "Copy pairing link",
                &self.copy_button_state,
                ServerSnapshot::read(app).running,
                appearance,
                WorkspaceAction::CopyRemoteControlUrl,
            ));

        if view.rotate_confirmation_pending {
            controls.add_child(action_button(
                "Confirm rotate",
                &self.rotate_button_state,
                true,
                appearance,
                RemoteControlPageAction::ConfirmRotateAccessToken,
            ));
            controls.add_child(action_button(
                "Cancel",
                &self.cancel_rotate_button_state,
                true,
                appearance,
                RemoteControlPageAction::CancelRotateAccessToken,
            ));
        } else {
            controls.add_child(action_button(
                "Rotate",
                &self.rotate_button_state,
                true,
                appearance,
                RemoteControlPageAction::BeginRotateAccessToken,
            ));
        }

        let description = if view.rotate_confirmation_pending {
            "Rotating invalidates every pairing URL handed out so far. Click Confirm rotate to continue."
        } else {
            "The long-lived secret a browser presents once to pair with this machine."
        };

        render_body_item::<RemoteControlPageAction>(
            "Access token".into(),
            None,
            ToggleState::Enabled,
            appearance,
            controls.finish(),
            Some(description.to_owned()),
        )
    }
}

#[derive(Default)]
struct LanAccessWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for LanAccessWidget {
    type View = RemoteControlSettingsPageView;

    fn search_terms(&self) -> &str {
        "remote control lan network other devices tailscale ssh tunnel http"
    }

    fn should_render(&self, _app: &AppContext) -> bool {
        remote_control_page_is_available()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let allows_lan_access = RemoteControlSettings::as_ref(app).allows_lan_access();
        let mut column = Flex::column();
        column.add_child(render_body_item::<RemoteControlPageAction>(
            "Allow access from other devices on this network".into(),
            None,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(allows_lan_access)
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(RemoteControlPageAction::ToggleLanAccess);
                })
                .finish(),
            Some(
                "Traffic is plain HTTP and is not encrypted on the wire. Prefer Tailscale, or an SSH tunnel such as `ssh -L 7777:127.0.0.1:7777`, over opening this to your network."
                    .to_owned(),
            ),
        ));

        if allows_lan_access {
            for url in ServerSnapshot::read(app).lan_urls {
                column.add_child(render_body_item::<RemoteControlPageAction>(
                    "Network URL".into(),
                    None,
                    ToggleState::Enabled,
                    appearance,
                    read_only_row(url, appearance),
                    None,
                ));
            }
        }

        if let Some(paired_devices) = view.render_paired_devices_slot() {
            column.add_child(paired_devices);
        }

        column.finish()
    }
}
