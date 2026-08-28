use std::sync::Arc;

use itertools::Itertools;
use markdown_parser::{FormattedText, FormattedTextFragment, FormattedTextLine};
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use thousands::Separable;
use warp_core::features::FeatureFlag;
use warp_core::ui::appearance::Appearance;
use warp_graphql::billing::AddonCreditsOption;
use warpui::elements::{
    Align, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex,
    FormattedTextElement, HighlightedHyperlink, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, Shrinkable, Text, Wrap,
};
use warpui::fonts::{Properties, Weight};
use warpui::prelude::ChildView;
use warpui::ui_components::button::{ButtonVariant, TextAndIcon, TextAndIconAlignment};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, UpdateView, View, ViewContext,
    ViewHandle, WeakViewHandle,
};

use super::billing_and_usage::overage_limit_modal::{SpendingLimitModal, SpendingLimitModalEvent};
pub use super::billing_and_usage_page::BillingAndUsagePageEvent;
use super::billing_and_usage_page::{
    BillingAndUsagePageAction, CHECKOUT_PENDING_MESSAGE, render_premium_upgrade_savings_note,
};
use super::plan_header_presentation;
use super::settings_page::{AdditionalInfo, render_customer_type_badge, render_info_icon};
use crate::auth::auth_state::AuthState;
use crate::auth::auth_view_modal::AuthViewVariant;
use crate::auth::{AuthManager, AuthStateProvider};
use crate::modal::{Modal, ModalEvent, ModalViewState};
use crate::pricing::PricingInfoModel;
use crate::send_telemetry_from_ctx;
use crate::server::ids::ServerId;
use crate::server::telemetry::TelemetryEvent;
use crate::ui_components::blended_colors;
use crate::ui_components::buttons::icon_button;
use crate::ui_components::icons::Icon;
use crate::view_components::ToastFlavor;
use crate::view_components::action_button::{ActionButton, PrimaryTheme, SecondaryTheme};
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::workspaces::user_workspaces::{UserWorkspaces, UserWorkspacesEvent};
use crate::workspaces::workspace::{CustomerType, Workspace};

const ADDON_CREDITS_DESCRIPTION: &str = "Add-on credits are purchased in prepaid packages that roll over each billing cycle and expire after one year. The more you purchase, the better the per-credit rate. Once your base plan credits are used, add-on credits will be consumed.";
const ADDITIONAL_ADDON_CREDITS_DESCRIPTION_FOR_TEAM: &str =
    "Purchased add-on credits are added to your team's shared credit pool.";
const MANAGED_AUTO_RELOAD_HEADER: &str = "Auto-reload is enabled";

const ADDON_CREDITS_DELINQUENT_WARNING_STRING: &str =
    "Restricted due to billing issue. Update your payment method to purchase add-on credits.";
const ADDON_CREDITS_NON_ADMIN_DELINQUENT_WARNING_STRING: &str =
    "Restricted due to billing issue. Contact your team admin to update their payment method.";
const RESTRICTED_BILLING_USAGE_WARNING_STRING: &str = "Auto reload is disabled due to recent failed reload. Please update your payment method and try again.";
const RESTRICTED_BILLING_USAGE_NON_ADMIN_WARNING_STRING: &str = "Auto reload is disabled due to recent failed reload. Contact your team admin to update their payment method.";

const HEADER_FONT_SIZE: f32 = 16.;

const DEFAULT_MAX_MONTHLY_SPEND_CENTS: i32 = 20_000;

#[derive(Default)]
struct PlanSectionMouseStates {
    manage_billing_link: MouseStateHandle,
    open_admin_panel_link: MouseStateHandle,
    admin_panel_link: MouseStateHandle,
    refresh_button: MouseStateHandle,
}

#[derive(Default)]
struct BuyCreditsMouseStates {
    addon_info_icon: MouseStateHandle,
    edit_monthly_limit: MouseStateHandle,
    auto_reload_switch: SwitchStateHandle,
    auto_reload_info: MouseStateHandle,
    buy_button: MouseStateHandle,
}
struct AddonCreditsState {
    selected_denomination: usize,
    options: Vec<AddonCreditsOption>,
    denomination_buttons: Vec<ViewHandle<ActionButton>>,
    purchase_loading: bool,
}
enum AddonCreditsPanelState {
    IneligiblePlan(AddonCreditsRestriction),
    AutoreloadNonAdmin {
        description_text: String,
        warning_text: Option<&'static str>,
    },
    Purchase(AddonCreditsPurchaseState),
}

enum AddonCreditsRestriction {
    UpgradeToBuild {
        link_text: &'static str,
        url: String,
    },
    ContactAccountExecutive,
    ContactTeamAdmin,
}

struct AddonCreditsPurchaseState {
    description_text: String,
    auto_reload_enabled: bool,
    has_admin_permissions: bool,
    /// Whether team-level purchase settings (spend limit, auto-reload) can be
    /// edited: requires admin permission AND an existing team. Teamless users
    /// can purchase, but have no team settings until their first purchase
    /// creates a team server-side.
    can_edit_team_settings: bool,
    purchase_disabled: bool,
    auto_reload_switch_disabled: bool,
    price_label: String,
    auto_reload_tooltip_text: String,
    warning_text: Option<&'static str>,
    /// Surcharge in basis points applied to displayed prices (0 = none).
    premium_bps: i32,
}

pub struct BillingAndUsagePageV2View {
    self_handle: WeakViewHandle<Self>,
    auth_state: Arc<AuthState>,
    addon_credit_modal_state: ModalViewState<Modal<SpendingLimitModal>>,
    addon_credits: AddonCreditsState,
    pending_auto_reload_toast: Option<String>,
    plan_mouse_states: PlanSectionMouseStates,
    buy_credits_mouse_states: BuyCreditsMouseStates,
}

impl BillingAndUsagePageV2View {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _handle, event, ctx| {
            me.handle_workspaces_event(event, ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(&AuthManager::handle(ctx), |me, _, _, ctx| {
            me.refresh_addon_credits_settings(ctx);
            ctx.notify();
        });

        let team_update_manager = TeamUpdateManager::handle(ctx);
        ctx.subscribe_to_model(&team_update_manager, |_, _handle, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &PricingInfoModel::handle(ctx),
            |me, _handle, _event, ctx| {
                me.update_addon_credits_options(ctx);
                me.refresh_addon_credits_settings(ctx);
                ctx.notify();
            },
        );

        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        let addon_credit_modal = ctx.add_typed_action_view(SpendingLimitModal::new);
        ctx.subscribe_to_view(&addon_credit_modal, |me, _, event, ctx| {
            me.handle_addon_credit_modal_event(event, ctx);
        });

        let addon_credit_modal_view = ctx.add_typed_action_view(|ctx| {
            Modal::new(
                Some("Monthly spending limit".to_string()),
                addon_credit_modal,
                ctx,
            )
            .with_header_style(UiComponentStyles {
                padding: Some(Coords::uniform(24.).bottom(16.)),
                ..Default::default()
            })
            .with_body_style(UiComponentStyles {
                padding: Some(Coords::uniform(24.).top(0.).bottom(12.)),
                ..Default::default()
            })
        });
        ctx.subscribe_to_view(&addon_credit_modal_view, |me, _, event, ctx| {
            me.handle_addon_credit_modal_close_event(event, ctx);
        });

        let mut me = Self {
            self_handle: ctx.handle(),
            auth_state,
            addon_credit_modal_state: ModalViewState::new(addon_credit_modal_view),
            addon_credits: AddonCreditsState {
                selected_denomination: 0,
                options: Default::default(),
                denomination_buttons: Default::default(),
                purchase_loading: false,
            },
            pending_auto_reload_toast: None,
            plan_mouse_states: Default::default(),
            buy_credits_mouse_states: Default::default(),
        };
        me.update_addon_credits_options(ctx);
        me.refresh_addon_credits_settings(ctx);
        me
    }

    fn refresh_addon_credits_settings(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(workspace) = UserWorkspaces::as_ref(ctx).current_workspace() else {
            return;
        };
        let addon_credits_settings = &workspace.settings.addon_credits_settings;
        if addon_credits_settings.auto_reload_enabled {
            self.addon_credits.selected_denomination = addon_credits_settings
                .selected_auto_reload_credit_denomination
                .and_then(|amount| {
                    self.addon_credits
                        .options
                        .iter()
                        .find_position(|option| option.credits == amount)
                })
                .map_or(0, |pair| pair.0);
        }
        self.update_denomination_buttons_focus(ctx);
    }

    fn update_denomination_buttons_focus(&mut self, ctx: &mut ViewContext<Self>) {
        for (i, button_handle) in self.addon_credits.denomination_buttons.iter().enumerate() {
            ctx.update_view(button_handle, |button, ctx| {
                if i == self.addon_credits.selected_denomination {
                    button.set_theme(PrimaryTheme, ctx);
                } else {
                    button.set_theme(SecondaryTheme, ctx);
                }
            });
        }
    }

    fn handle_workspaces_event(
        &mut self,
        event: &UserWorkspacesEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            UserWorkspacesEvent::TeamsChanged => {
                self.update_addon_credit_modal(ctx);
            }
            UserWorkspacesEvent::UpdateWorkspaceSettingsSuccess => {
                self.update_addon_credit_modal(ctx);
                self.refresh_addon_credits_settings(ctx);
                if let Some(message) = self.pending_auto_reload_toast.take() {
                    self.show_toast(&message, ToastFlavor::Success, ctx);
                }
                ctx.notify();
            }
            UserWorkspacesEvent::UpdateWorkspaceSettingsRejected(_err) => {
                self.pending_auto_reload_toast = None;
                self.show_toast(
                    "Failed to update workspace settings",
                    ToastFlavor::Error,
                    ctx,
                );
            }
            UserWorkspacesEvent::AiOveragesUpdated => {
                ctx.notify();
            }
            UserWorkspacesEvent::PurchaseAddonCreditsSuccess => {
                self.addon_credits.purchase_loading = false;
                self.show_toast(
                    "Successfully purchased add-on credits",
                    ToastFlavor::Success,
                    ctx,
                );
            }
            UserWorkspacesEvent::PurchaseAddonCreditsCheckoutRequired { checkout_url } => {
                if self.addon_credits.purchase_loading {
                    self.addon_credits.purchase_loading = false;
                    ctx.open_url(checkout_url);
                    self.show_toast(CHECKOUT_PENDING_MESSAGE, ToastFlavor::Default, ctx);
                    // Credits are granted via webhook once checkout completes;
                    // `on_page_selected` refreshes billing data when the user
                    // returns (e.g. via the confirmation page's Open Warp link).
                }
            }
            UserWorkspacesEvent::PurchaseAddonCreditsRejected(err) => {
                self.addon_credits.purchase_loading = false;
                self.show_toast(&err.to_string(), ToastFlavor::Error, ctx);
            }
            _ => {}
        }
    }

    fn show_toast(&self, message: &str, flavor: ToastFlavor, ctx: &mut ViewContext<Self>) {
        ctx.emit(BillingAndUsagePageEvent::ShowToast {
            message: message.to_string(),
            flavor,
        });
    }

    pub fn get_modal_content(&self) -> Option<Box<dyn Element>> {
        if self.addon_credit_modal_state.is_open() {
            Some(self.addon_credit_modal_state.render())
        } else {
            None
        }
    }

    fn handle_addon_credit_modal_close_event(
        &mut self,
        event: &ModalEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            ModalEvent::Close => {
                self.addon_credit_modal_state.close();
                ctx.emit(BillingAndUsagePageEvent::HideModal);
            }
        }
    }

    fn handle_addon_credit_modal_event(
        &mut self,
        event: &SpendingLimitModalEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            SpendingLimitModalEvent::Close => {
                self.hide_addon_credit_modal(ctx);
            }
            SpendingLimitModalEvent::Update { amount_cents } => {
                let workspaces = UserWorkspaces::as_ref(ctx);
                let team_uid = workspaces.team_uid_for_window(ctx.window_id());

                if let Some(team_uid) = team_uid {
                    UserWorkspaces::handle(ctx).update(ctx, |user_workspaces, ctx| {
                        user_workspaces.update_addon_credits_settings(
                            team_uid,
                            None,
                            Some(*amount_cents as i32),
                            None,
                            ctx,
                        );
                    });
                    self.hide_addon_credit_modal(ctx);
                    ctx.notify();
                }
            }
        }
    }

    fn show_addon_credit_modal(&mut self, ctx: &mut ViewContext<Self>) {
        self.addon_credit_modal_state.open();
        self.addon_credit_modal_state
            .view
            .update(ctx, |modal, ctx| {
                modal.body().update(ctx, |body, ctx| {
                    body.focus_input(ctx);
                });
            });
        ctx.emit(BillingAndUsagePageEvent::ShowModal);
    }

    fn hide_addon_credit_modal(&mut self, ctx: &mut ViewContext<Self>) {
        self.addon_credit_modal_state.close();
        ctx.emit(BillingAndUsagePageEvent::HideModal);
    }

    fn update_addon_credit_modal(&mut self, ctx: &mut ViewContext<Self>) {
        let addon_limit = UserWorkspaces::as_ref(ctx)
            .current_workspace()
            .and_then(|ws| ws.settings.addon_credits_settings.max_monthly_spend_cents)
            .unwrap_or(DEFAULT_MAX_MONTHLY_SPEND_CENTS);

        self.addon_credit_modal_state
            .view
            .update(ctx, |modal, ctx| {
                modal.body().update(ctx, |body, ctx| {
                    body.update_amount_editor(addon_limit.max(0) as u32, ctx);
                });
            });
        ctx.notify();
    }

    fn update_addon_credits_options(&mut self, ctx: &mut ViewContext<Self>) {
        self.addon_credits.options = PricingInfoModel::as_ref(ctx)
            .addon_credits_options()
            .map(|options| options.to_vec())
            .unwrap_or_default();
        self.addon_credits.denomination_buttons = self
            .addon_credits
            .options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                ctx.add_typed_action_view(move |_ctx| {
                    ActionButton::new(option.credits.separate_with_commas(), SecondaryTheme)
                        .with_icon(Icon::Credits)
                        .on_click(move |ctx| {
                            ctx.dispatch_typed_action(
                                BillingAndUsagePageAction::SelectTopupDenomination(i),
                            );
                        })
                })
            })
            .collect();
    }

    // ── Rendering ────────────────────────────────────────────────────────

    fn render_plan_section(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let mut plan_header = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max);

        plan_header.add_child(
            Text::new_inline("Plan", appearance.ui_font_family(), HEADER_FONT_SIZE)
                .with_style(Properties::default().weight(Weight::Bold))
                .with_color(appearance.theme().active_ui_text_color().into())
                .finish(),
        );

        let mut right_side = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::End);

        let workspaces = UserWorkspaces::as_ref(app);
        let workspace = workspaces.current_workspace();
        let billing_metadata = workspace.map(|workspace| &workspace.billing_metadata);
        let team = workspaces.team_for_view_handle(&self.self_handle, app);
        let presentation = plan_header_presentation(billing_metadata, team.is_some(), false);
        if let Some(badge_label) = presentation.badge_label {
            right_side.add_child(
                Container::new(render_customer_type_badge(appearance, badge_label))
                    .with_margin_right(8.)
                    .finish(),
            );
        }
        if let Some(team) = team {
            let current_user_email = AuthStateProvider::as_ref(app)
                .get()
                .user_email()
                .unwrap_or_default();
            let is_team_admin = team.has_admin_permissions(&current_user_email);
            let is_workspace_admin = workspace
                .is_some_and(|workspace| workspace.is_workspace_admin(&current_user_email));

            if is_team_admin
                && billing_metadata.is_some_and(|billing_metadata| {
                    billing_metadata.customer_type != CustomerType::Enterprise
                })
                && workspace.is_some_and(|workspace| workspace.has_billing_history)
            {
                let team_uid = team.uid;
                let fg_color = appearance.theme().active_ui_text_color();
                right_side.add_child(
                    Container::new(
                        appearance
                            .ui_builder()
                            .button(
                                ButtonVariant::Link,
                                self.plan_mouse_states.manage_billing_link.clone(),
                            )
                            .with_text_and_icon_label(
                                TextAndIcon::new(
                                    TextAndIconAlignment::IconFirst,
                                    "Manage billing",
                                    Icon::CoinsStacked.to_warpui_icon(fg_color),
                                    MainAxisSize::Min,
                                    MainAxisAlignment::Center,
                                    vec2f(14., 14.),
                                )
                                .with_inner_padding(4.),
                            )
                            .with_style(UiComponentStyles {
                                font_color: Some(fg_color.into()),
                                ..Default::default()
                            })
                            .build()
                            .on_click(move |ctx, _, _| {
                                ctx.dispatch_typed_action(
                                    BillingAndUsagePageAction::GenerateStripeBillingPortalLink {
                                        team_uid,
                                    },
                                );
                            })
                            .finish(),
                    )
                    .with_margin_left(8.)
                    .finish(),
                );
            }

            if should_show_open_admin_panel_link(
                is_team_admin,
                is_workspace_admin,
                billing_metadata.is_some_and(|metadata| metadata.is_enterprise_plan()),
            ) {
                let team_uid = team.uid;
                let use_workspace_admin_panel = workspace.is_some_and(|workspace| {
                    workspace.is_native_workspaces_admin(&current_user_email)
                });
                let fg_color = appearance.theme().active_ui_text_color();
                right_side.add_child(
                    Container::new(
                        appearance
                            .ui_builder()
                            .button(
                                ButtonVariant::Link,
                                self.plan_mouse_states.open_admin_panel_link.clone(),
                            )
                            .with_text_and_icon_label(
                                TextAndIcon::new(
                                    TextAndIconAlignment::IconFirst,
                                    "Open admin panel",
                                    Icon::Users.to_warpui_icon(fg_color),
                                    MainAxisSize::Min,
                                    MainAxisAlignment::Center,
                                    vec2f(14., 14.),
                                )
                                .with_inner_padding(4.),
                            )
                            .with_style(UiComponentStyles {
                                font_color: Some(fg_color.into()),
                                ..Default::default()
                            })
                            .build()
                            .on_click(move |ctx, _, _| {
                                if use_workspace_admin_panel {
                                    ctx.dispatch_typed_action(
                                        BillingAndUsagePageAction::OpenWorkspaceAdminPanel,
                                    );
                                } else {
                                    ctx.dispatch_typed_action(
                                        BillingAndUsagePageAction::OpenTeamAdminPanel { team_uid },
                                    );
                                }
                            })
                            .finish(),
                    )
                    .with_margin_left(8.)
                    .finish(),
                );
            }
        } else if presentation.show_personal_upgrade {
            let current_user_id = self.auth_state.user_id().unwrap_or_default();
            right_side.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .button(
                            ButtonVariant::Link,
                            self.plan_mouse_states.admin_panel_link.clone(),
                        )
                        .with_text_and_icon_label(
                            TextAndIcon::new(
                                TextAndIconAlignment::IconFirst,
                                "Compare plans",
                                Icon::CoinsStacked
                                    .to_warpui_icon(appearance.theme().active_ui_text_color()),
                                MainAxisSize::Min,
                                MainAxisAlignment::Center,
                                vec2f(14., 14.),
                            )
                            .with_inner_padding(4.),
                        )
                        .with_style(UiComponentStyles {
                            font_color: Some(appearance.theme().active_ui_text_color().into()),
                            ..Default::default()
                        })
                        .build()
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(BillingAndUsagePageAction::Upgrade {
                                team_uid: None,
                                user_id: current_user_id,
                            });
                        })
                        .finish(),
                )
                .with_margin_left(8.)
                .finish(),
            );
        }

        right_side.add_child(
            Container::new(self.render_plan_refresh_button(appearance))
                .with_margin_left(8.)
                .finish(),
        );

        plan_header.add_child(right_side.finish());

        Container::new(plan_header.finish())
            .with_margin_bottom(24.)
            .finish()
    }

    fn render_plan_refresh_button(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let icon_color = theme.sub_text_color(theme.background());
        let mouse_state = self.plan_mouse_states.refresh_button.clone();
        warpui::elements::Hoverable::new(mouse_state, move |_| {
            Container::new(
                ConstrainedBox::new(Icon::Refresh.to_warpui_icon(icon_color).finish())
                    .with_width(16.)
                    .with_height(16.)
                    .finish(),
            )
            .with_uniform_padding(2.)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(BillingAndUsagePageAction::RefreshWorkspaceData);
        })
        .finish()
    }

    fn render_addon_credits_panel(
        &self,
        workspace: Option<&Workspace>,
        team_uid: Option<ServerId>,
        has_admin_permissions: bool,
        delinquent: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        match self.addon_credits_panel_state(
            workspace,
            team_uid,
            has_admin_permissions,
            delinquent,
            app,
        ) {
            AddonCreditsPanelState::IneligiblePlan(restriction) => {
                self.render_addon_credits_ineligible_plan_card(restriction, appearance)
            }
            AddonCreditsPanelState::AutoreloadNonAdmin {
                description_text,
                warning_text,
            } => self.render_addon_credits_non_admin_auto_reload_card(
                appearance,
                description_text,
                warning_text,
            ),
            AddonCreditsPanelState::Purchase(state) => {
                self.render_addon_credits_purchase_card(workspace, team_uid, state, appearance)
            }
        }
    }

    fn addon_credits_panel_state(
        &self,
        workspace: Option<&Workspace>,
        team_uid: Option<ServerId>,
        has_admin_permissions: bool,
        delinquent: bool,
        app: &AppContext,
    ) -> AddonCreditsPanelState {
        let workspaces = UserWorkspaces::as_ref(app);
        let purchase_policy = workspaces.purchase_policy();
        let team_can_purchase = purchase_policy.is_some_and(|policy| policy.allows_purchases());
        let premium_bps = purchase_policy.map_or(0, |policy| policy.effective_premium_bps());
        let can_upgrade = workspace
            .is_none_or(|workspace| workspace.billing_metadata.can_upgrade_to_build_plan());
        let upgrade_url = match team_uid {
            Some(team_uid) => UserWorkspaces::upgrade_link_for_team(team_uid),
            None => UserWorkspaces::upgrade_link(self.auth_state.user_id().unwrap_or_default()),
        };

        if !team_can_purchase {
            if !has_admin_permissions {
                return AddonCreditsPanelState::IneligiblePlan(
                    AddonCreditsRestriction::ContactTeamAdmin,
                );
            } else if can_upgrade {
                return AddonCreditsPanelState::IneligiblePlan(
                    AddonCreditsRestriction::UpgradeToBuild {
                        link_text: "Upgrade to Build",
                        url: upgrade_url,
                    },
                );
            }
            return AddonCreditsPanelState::IneligiblePlan(
                AddonCreditsRestriction::ContactAccountExecutive,
            );
        }

        let selected_credit_option = self
            .addon_credits
            .options
            .get(self.addon_credits.selected_denomination);
        let auto_reload_enabled = workspace.is_some_and(|workspace| {
            workspace
                .settings
                .addon_credits_settings
                .auto_reload_enabled
        });

        let team_count = workspaces
            .team_for_view_handle(&self.self_handle, app)
            .map(|team| team.members.len())
            .unwrap_or(1);
        let description_text = if team_count > 1 {
            format!("{ADDON_CREDITS_DESCRIPTION} {ADDITIONAL_ADDON_CREDITS_DESCRIPTION_FOR_TEAM}")
        } else {
            ADDON_CREDITS_DESCRIPTION.to_string()
        };

        let would_exceed = workspace
            .zip(selected_credit_option)
            .is_some_and(|(workspace, opt)| {
                let limit = workspace
                    .settings
                    .addon_credits_settings
                    .max_monthly_spend_cents
                    .unwrap_or(DEFAULT_MAX_MONTHLY_SPEND_CENTS);
                (workspace.bonus_grants_purchased_this_month.cents_spent
                    + opt.price_usd_cents_with_premium(premium_bps))
                    > limit
            });
        let purchase_disabled = self.addon_credits.purchase_loading
            || would_exceed
            || delinquent
            || auto_reload_enabled;
        let auto_reload_switch_disabled = !has_admin_permissions
            || delinquent
            || (!auto_reload_enabled && selected_credit_option.is_none());
        let price_label = selected_credit_option
            .map(|opt| {
                let credits = opt.credits.separate_with_commas();
                let dollars = format!(
                    "${:.2}",
                    opt.price_usd_cents_with_premium(premium_bps) as f64 / 100.0
                );
                format!("{credits} credits / {dollars}")
            })
            .unwrap_or_default();
        let auto_reload_credit_amount = selected_credit_option
            .map(|o| format!("{} credits", o.credits.separate_with_commas()))
            .unwrap_or_else(|| "selected credit amount".to_string());
        let auto_reload_tooltip_text = format!(
            "When any member on your team’s credit balance reaches 100 credits remaining, \
            automatically purchase {auto_reload_credit_amount}."
        );
        let warning_text = if delinquent && has_admin_permissions {
            Some(ADDON_CREDITS_DELINQUENT_WARNING_STRING)
        } else if delinquent {
            Some(ADDON_CREDITS_NON_ADMIN_DELINQUENT_WARNING_STRING)
        } else if workspace.is_some_and(|workspace| {
            workspace
                .billing_metadata
                .has_failed_addon_credit_auto_reload_status()
        }) {
            Some(if has_admin_permissions {
                RESTRICTED_BILLING_USAGE_WARNING_STRING
            } else {
                RESTRICTED_BILLING_USAGE_NON_ADMIN_WARNING_STRING
            })
        } else if would_exceed {
            Some(match (auto_reload_enabled, has_admin_permissions) {
                (true, true) => {
                    "Auto-reload is paused because the next reload would exceed your monthly spend limit. Increase your limit to continue using auto-reload."
                }
                (true, false) => {
                    "Auto-reload is paused because the next reload would exceed your team’s monthly spend limit. Contact a team admin to increase it."
                }
                (false, true) => {
                    "This purchase would exceed your monthly limit. Increase your limit to continue."
                }
                (false, false) => {
                    "This purchase would exceed your team’s monthly spend limit. Contact a team admin to increase it."
                }
            })
        } else {
            None
        };

        if !has_admin_permissions && auto_reload_enabled {
            let configured_auto_reload_option = workspace
                .and_then(|workspace| {
                    workspace
                        .settings
                        .addon_credits_settings
                        .selected_auto_reload_credit_denomination
                })
                .and_then(|credits| {
                    self.addon_credits
                        .options
                        .iter()
                        .find(|option| option.credits == credits)
                })
                .or(selected_credit_option);
            let description_text = match configured_auto_reload_option {
                Some(option) => {
                    let credits = option.credits.separate_with_commas();
                    let price = format!(
                        "${:.2}",
                        option.price_usd_cents_with_premium(premium_bps) as f64 / 100.0
                    );
                    format!(
                        "Your admin has enabled auto-reload for add-on credits. When your team's add-on credit balance runs low, Warp will automatically purchase {credits} credits for {price} and add them to your team's shared pool."
                    )
                }
                None => {
                    "Your admin has enabled auto-reload for add-on credits. When your team's add-on credit balance runs low, Warp will automatically purchase add-on credits and add them to your team's shared pool.".to_string()
                }
            };
            return AddonCreditsPanelState::AutoreloadNonAdmin {
                description_text,
                warning_text,
            };
        }

        AddonCreditsPanelState::Purchase(AddonCreditsPurchaseState {
            description_text,
            auto_reload_enabled,
            has_admin_permissions,
            can_edit_team_settings: has_admin_permissions && team_uid.is_some(),
            purchase_disabled,
            auto_reload_switch_disabled,
            price_label,
            auto_reload_tooltip_text,
            warning_text,
            premium_bps,
        })
    }

    fn render_addon_credits_ineligible_plan_card(
        &self,
        restriction: AddonCreditsRestriction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let bg = theme.background();
        let explanation = match restriction {
            AddonCreditsRestriction::UpgradeToBuild { link_text, url } => {
                FormattedTextElement::new(
                    FormattedText::new([FormattedTextLine::Line(vec![
                        FormattedTextFragment::hyperlink(link_text, url),
                        FormattedTextFragment::plain_text(" to purchase add-on credits."),
                    ])]),
                    appearance.ui_font_size(),
                    appearance.ui_font_family(),
                    appearance.ui_font_family(),
                    theme.sub_text_color(bg).into(),
                    HighlightedHyperlink::default(),
                )
                .with_hyperlink_font_color(theme.accent().into_solid())
                .register_default_click_handlers_with_action_support(
                    |lens, event, ctx| match lens {
                        warpui::elements::HyperlinkLens::Url(u) => ctx.open_url(u),
                        warpui::elements::HyperlinkLens::Action(a) => {
                            if let Some(act) =
                                a.as_any().downcast_ref::<BillingAndUsagePageAction>()
                            {
                                event.dispatch_typed_action(act.clone());
                            }
                        }
                    },
                )
                .finish()
            }
            AddonCreditsRestriction::ContactAccountExecutive => appearance
                .ui_builder()
                .paragraph("Contact your Account Executive for more add-on credits.")
                .with_style(UiComponentStyles {
                    font_color: Some(theme.sub_text_color(bg).into()),
                    ..Default::default()
                })
                .build()
                .finish(),
            AddonCreditsRestriction::ContactTeamAdmin => appearance
                .ui_builder()
                .paragraph("Contact a team admin to enable add-on credits.")
                .with_style(UiComponentStyles {
                    font_color: Some(theme.sub_text_color(bg).into()),
                    ..Default::default()
                })
                .build()
                .finish(),
        };
        let header = Text::new_inline("Buy credits", appearance.ui_font_family(), HEADER_FONT_SIZE)
            .with_color(theme.foreground().into())
            .with_style(Properties::default().weight(Weight::Medium))
            .finish();
        let card = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_children([
                Container::new(header).with_margin_bottom(8.).finish(),
                explanation,
            ])
            .finish();

        Container::new(card)
            .with_background_color(theme.surface_1().into_solid())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .with_margin_bottom(16.)
            .with_uniform_padding(16.)
            .finish()
    }

    fn render_addon_credits_non_admin_auto_reload_card(
        &self,
        appearance: &Appearance,
        description_text: String,
        warning_text: Option<&'static str>,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let bg = theme.background();
        let auto_reload_header = Text::new_inline(
            MANAGED_AUTO_RELOAD_HEADER,
            appearance.ui_font_family(),
            HEADER_FONT_SIZE,
        )
        .with_color(theme.foreground().into())
        .with_style(Properties::default().weight(Weight::Medium))
        .finish();
        let auto_reload_description = appearance
            .ui_builder()
            .paragraph(description_text)
            .with_style(UiComponentStyles {
                font_color: Some(theme.sub_text_color(bg).into()),
                ..Default::default()
            })
            .build()
            .finish();
        let mut card_children = vec![auto_reload_header, auto_reload_description];
        if let Some(warning_text) = warning_text {
            card_children.push(self.render_warning_row(appearance, warning_text.to_string()));
        }
        let card = Flex::column()
            .with_children(card_children)
            .with_spacing(8.)
            .finish();

        Container::new(card)
            .with_background_color(theme.surface_1().into_solid())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .with_margin_bottom(16.)
            .with_uniform_padding(16.)
            .finish()
    }

    fn render_addon_credits_purchase_card(
        &self,
        workspace: Option<&Workspace>,
        team_uid: Option<ServerId>,
        state: AddonCreditsPurchaseState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let card_upper = self.render_addon_credits_upper_section(workspace, &state, appearance);
        let card_lower = self.render_addon_credits_lower_section(team_uid, &state, appearance);

        Container::new(
            Flex::column()
                .with_children([card_upper, card_lower])
                .finish(),
        )
        .with_background_color(theme.surface_1().into_solid())
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .with_margin_bottom(16.)
        .finish()
    }

    fn render_addon_credits_upper_section(
        &self,
        workspace: Option<&Workspace>,
        state: &AddonCreditsPurchaseState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let bg = theme.background();
        let ui_builder = appearance.ui_builder();
        let header = Text::new_inline("Buy credits", appearance.ui_font_family(), HEADER_FONT_SIZE)
            .with_color(theme.foreground().into())
            .with_style(Properties::default().weight(Weight::Medium))
            .finish();
        let paragraph = ui_builder
            .paragraph(state.description_text.clone())
            .with_style(UiComponentStyles {
                font_color: Some(theme.sub_text_color(bg).into()),
                ..Default::default()
            })
            .build()
            .finish();
        let mut upper_section = Flex::column()
            .with_children([header, paragraph])
            .with_spacing(8.);

        if state.can_edit_team_settings {
            let info_icon = render_info_icon(
                appearance,
                AdditionalInfo::<BillingAndUsagePageAction> {
                    mouse_state: self.buy_credits_mouse_states.addon_info_icon.clone(),
                    on_click_action: None,
                    secondary_text: None,
                    tooltip_override_text: Some(
                        "Sets the monthly limit spent on add-on credits".to_string(),
                    ),
                },
            );
            let spend_limit = workspace
                .and_then(|workspace| {
                    workspace
                        .settings
                        .addon_credits_settings
                        .max_monthly_spend_cents
                })
                .map(|c| format!("${:.2}", c as f64 / 100.0))
                .unwrap_or_else(|| "$200.00".to_string());
            let spend_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_children([
                    ui_builder.span("Monthly spend limit").build().finish(),
                    Shrinkable::new(1., Align::new(info_icon).left().finish()).finish(),
                    icon_button(
                        appearance,
                        Icon::Pencil,
                        false,
                        self.buy_credits_mouse_states.edit_monthly_limit.clone(),
                    )
                    .build()
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(BillingAndUsagePageAction::ShowAddOnCreditModal);
                    })
                    .finish(),
                    ui_builder.span(spend_limit).build().finish(),
                ])
                .finish();
            upper_section.add_child(spend_row);

            if let Some(purchased_row) = workspace
                .and_then(|workspace| Self::render_purchased_this_month_row(workspace, appearance))
            {
                upper_section.add_child(purchased_row);
            }
        }

        if state.has_admin_permissions || !state.auto_reload_enabled {
            let denomination_button_elements = self
                .addon_credits
                .denomination_buttons
                .iter()
                .map(|button_handle| ChildView::new(button_handle).finish())
                .collect::<Vec<_>>();
            upper_section.add_child(
                Container::new(
                    Wrap::row()
                        .with_children(denomination_button_elements)
                        .with_spacing(8.)
                        .finish(),
                )
                .finish(),
            );
        }

        Container::new(upper_section.finish())
            .with_horizontal_padding(16.)
            .with_padding_top(16.)
            .with_padding_bottom(16.)
            .finish()
    }

    fn render_purchased_this_month_row(
        workspace: &Workspace,
        appearance: &Appearance,
    ) -> Option<Box<dyn Element>> {
        let bonus_grants = &workspace.bonus_grants_purchased_this_month;
        if bonus_grants.total_credits_purchased == 0 {
            return None;
        }

        let credits_purchased = bonus_grants.total_credits_purchased;
        let cost_dollars = bonus_grants.cents_spent as f64 / 100.0;
        let theme = appearance.theme();

        let label = Text::new_inline("Purchased this month", appearance.ui_font_family(), 12.)
            .with_color(theme.active_ui_text_color().into())
            .finish();

        let credits_text = if credits_purchased == 1 {
            "1 credit".to_string()
        } else {
            format!("{} credits", credits_purchased.separate_with_commas())
        };

        let credits_component = Container::new(
            Text::new_inline(credits_text, appearance.ui_font_family(), 12.)
                .with_color(blended_colors::text_disabled(theme, theme.surface_1()))
                .finish(),
        )
        .with_margin_right(8.)
        .finish();

        let cost_component = Text::new_inline(
            format!("${cost_dollars:.2}"),
            appearance.ui_font_family(),
            12.,
        )
        .with_color(blended_colors::text_sub(theme, theme.surface_1()))
        .finish();

        Some(
            Container::new(
                Flex::row()
                    .with_child(label)
                    .with_child(
                        Flex::row()
                            .with_child(credits_component)
                            .with_child(cost_component)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .finish(),
                    )
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_main_axis_size(MainAxisSize::Max)
                    .finish(),
            )
            .with_margin_bottom(4.)
            .finish(),
        )
    }

    fn render_addon_credits_lower_section(
        &self,
        team_uid: Option<ServerId>,
        state: &AddonCreditsPurchaseState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let fg = theme.foreground();
        let auto_reload_enabled = state.auto_reload_enabled;
        let purchase_button_label = if self.addon_credits.purchase_loading {
            "Buying\u{2026}"
        } else {
            "One-time purchase"
        };
        let purchase_button_font_color = state
            .purchase_disabled
            .then_some(theme.disabled_text_color(theme.surface_3()).into());
        let purchase_button_background_color =
            state.purchase_disabled.then_some(theme.surface_3().into());
        let purchase_button_border_color = state
            .purchase_disabled
            .then_some(ColorU::transparent_black().into());
        let mut purchase_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Accent,
                self.buy_credits_mouse_states.buy_button.clone(),
            )
            .with_text_label(purchase_button_label.to_string())
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                font_weight: Some(Weight::Semibold),
                font_color: purchase_button_font_color,
                background: purchase_button_background_color,
                border_color: purchase_button_border_color,
                ..Default::default()
            })
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(BillingAndUsagePageAction::PurchaseAddonCredits {
                    team_uid,
                });
            });
        if state.purchase_disabled {
            purchase_button = purchase_button.disable();
        }
        let purchase_button = purchase_button.finish();
        let price_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(state.price_label.clone(), appearance.ui_font_family(), 14.)
                    .with_color(fg.into())
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            );

        let mut right_group = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        if state.can_edit_team_settings {
            let auto_reload_switch_element = {
                let switch_builder = appearance
                    .ui_builder()
                    .switch(self.buy_credits_mouse_states.auto_reload_switch.clone())
                    .check(auto_reload_enabled);
                if state.auto_reload_switch_disabled {
                    switch_builder.disable().build().finish()
                } else {
                    switch_builder
                        .build()
                        .on_click(move |ctx, _, _| {
                            if let Some(team_uid) = team_uid {
                                ctx.dispatch_typed_action(
                                    BillingAndUsagePageAction::UpdateAutoReloadEnabled {
                                        team_uid,
                                        enabled: !auto_reload_enabled,
                                    },
                                );
                            }
                        })
                        .finish()
                }
            };
            let auto_reload_info_icon = render_info_icon(
                appearance,
                AdditionalInfo::<BillingAndUsagePageAction> {
                    mouse_state: self.buy_credits_mouse_states.auto_reload_info.clone(),
                    on_click_action: None,
                    secondary_text: None,
                    tooltip_override_text: Some(state.auto_reload_tooltip_text.clone()),
                },
            );

            right_group.add_children([
                Text::new_inline("Auto-reload", appearance.ui_font_family(), 14.)
                    .with_color(fg.into())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
                Container::new(auto_reload_info_icon)
                    .with_margin_left(4.)
                    .finish(),
                Container::new(auto_reload_switch_element)
                    .with_margin_left(8.)
                    .finish(),
            ]);
        }
        right_group.add_child(
            Container::new(purchase_button)
                .with_margin_left(if state.can_edit_team_settings {
                    16.
                } else {
                    0.
                })
                .finish(),
        );
        let lower_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(price_row.finish())
            .with_child(right_group.finish());
        let mut lower_children: Vec<Box<dyn Element>> = vec![lower_row.finish()];

        if state.premium_bps > 0 {
            let upgrade_url = match team_uid {
                Some(team_uid) => UserWorkspaces::upgrade_link_for_team(team_uid),
                None => UserWorkspaces::upgrade_link(self.auth_state.user_id().unwrap_or_default()),
            };
            lower_children.push(render_premium_upgrade_savings_note(
                upgrade_url,
                state.premium_bps,
                appearance,
            ));
        }

        if let Some(warning_text) = state.warning_text {
            lower_children.push(self.render_warning_row(appearance, warning_text.to_string()));
        }

        Container::new(
            Flex::column()
                .with_children(lower_children)
                .with_spacing(8.)
                .finish(),
        )
        .with_uniform_padding(16.)
        .with_border(Border::top(1.).with_border_color(theme.outline().into()))
        .finish()
    }

    fn render_warning_row(&self, appearance: &Appearance, msg: String) -> Box<dyn Element> {
        let theme = appearance.theme();
        let icon = ConstrainedBox::new(
            Icon::AlertTriangle
                .to_warpui_icon(theme.ui_error_color().into())
                .finish(),
        )
        .with_height(16.)
        .with_width(16.)
        .finish();
        let text = Text::new(msg, appearance.ui_font_family(), 12.)
            .with_color(theme.ui_error_color())
            .finish();
        Container::new(
            Flex::row()
                .with_child(Container::new(icon).with_margin_right(8.).finish())
                .with_child(Shrinkable::new(1.0, text).finish())
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .finish(),
        )
        .with_margin_top(8.)
        .finish()
    }

    fn render_overview_tab(&self, app: &AppContext) -> Box<dyn Element> {
        let mut content = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        let workspaces = UserWorkspaces::as_ref(app);
        let delinquent = workspaces.current_workspace().is_some_and(|workspace| {
            workspace
                .billing_metadata
                .is_delinquent_due_to_payment_issue()
        });

        let ws = workspaces.current_workspace();
        let team = workspaces.team_for_view_handle(&self.self_handle, app);
        let show_addon_credits_panel = ws.is_some()
            || workspaces
                .purchase_policy()
                .is_some_and(|policy| policy.allows_purchases());
        if show_addon_credits_panel {
            let current_user_is_admin = team.is_none_or(|team| {
                let email = AuthStateProvider::as_ref(app)
                    .get()
                    .user_email()
                    .unwrap_or_default();
                team.has_admin_permissions(&email)
            });
            content.add_child(self.render_addon_credits_panel(
                ws,
                team.map(|team| team.uid),
                current_user_is_admin,
                delinquent,
                app,
            ));
        }

        content.finish()
    }
}

impl BillingAndUsagePageV2View {
    pub(super) fn on_page_selected(
        &mut self,
        _allow_steal_focus: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        self.addon_credits.purchase_loading = false;
        std::mem::drop(
            TeamUpdateManager::handle(ctx)
                .update(ctx, |mgr, ctx| mgr.refresh_workspace_metadata(ctx)),
        );
        self.refresh_addon_credits_settings(ctx);
    }
}

impl Entity for BillingAndUsagePageV2View {
    type Event = BillingAndUsagePageEvent;
}

impl View for BillingAndUsagePageV2View {
    fn ui_name() -> &'static str {
        "Billing and usage v2"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut page = Flex::column();
        page.add_child(self.render_plan_section(appearance, app));

        page.add_child(self.render_overview_tab(app));

        page.finish()
    }
}

impl TypedActionView for BillingAndUsagePageV2View {
    type Action = BillingAndUsagePageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        let is_login_gated = matches!(
            action,
            BillingAndUsagePageAction::Upgrade { .. }
                | BillingAndUsagePageAction::GenerateStripeBillingPortalLink { .. },
        );
        if AuthStateProvider::as_ref(ctx)
            .get()
            .is_anonymous_or_logged_out()
            && is_login_gated
        {
            AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                auth_manager.attempt_login_gated_feature(
                    action.into(),
                    AuthViewVariant::RequireLoginCloseable,
                    ctx,
                )
            });
            return;
        }

        match action {
            BillingAndUsagePageAction::Upgrade { team_uid, user_id } => match team_uid {
                Some(team_uid) => ctx.open_url(&UserWorkspaces::upgrade_link_for_team(*team_uid)),
                None => ctx.open_url(&UserWorkspaces::upgrade_link(*user_id)),
            },
            BillingAndUsagePageAction::GenerateStripeBillingPortalLink { team_uid } => {
                UserWorkspaces::handle(ctx).update(ctx, |ws, ctx| {
                    ws.generate_stripe_billing_portal_link(*team_uid, ctx);
                });
            }
            BillingAndUsagePageAction::OpenTeamAdminPanel { team_uid } => {
                super::admin_actions::AdminActions::open_admin_panel(*team_uid, ctx);
            }
            BillingAndUsagePageAction::OpenWorkspaceAdminPanel => {
                super::admin_actions::AdminActions::open_workspace_admin_panel(ctx);
            }
            BillingAndUsagePageAction::ContactSupport => {
                super::admin_actions::AdminActions::contact_support(ctx);
            }
            BillingAndUsagePageAction::SignupAnonymousUser => {
                ctx.emit(BillingAndUsagePageEvent::SignupAnonymousUser);
            }
            BillingAndUsagePageAction::AttemptLoginGatedUpgrade => {
                AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                    auth_manager.attempt_login_gated_feature(
                        action.into(),
                        AuthViewVariant::RequireLoginCloseable,
                        ctx,
                    )
                });
            }
            BillingAndUsagePageAction::OpenUrl(url) => ctx.open_url(&url.url),
            // Not applicable in v2
            BillingAndUsagePageAction::UpdateUsageBasedPricingSettings { .. }
            | BillingAndUsagePageAction::ShowOverageLimitModal => {}
            BillingAndUsagePageAction::RefreshWorkspaceData => {
                std::mem::drop(
                    TeamUpdateManager::handle(ctx)
                        .update(ctx, |mgr, ctx| mgr.refresh_workspace_metadata(ctx)),
                );
            }
            BillingAndUsagePageAction::SelectTopupDenomination(i) => {
                self.addon_credits.selected_denomination = *i;
                self.update_denomination_buttons_focus(ctx);
                let workspaces = UserWorkspaces::as_ref(ctx);
                let team = workspaces.team_for_view(ctx);
                let has_admin_permissions = team.is_some_and(|team| {
                    AuthStateProvider::as_ref(ctx)
                        .get()
                        .user_email()
                        .is_some_and(|email| team.has_admin_permissions(&email))
                });
                let team_uid = team.map(|team| team.uid);
                UserWorkspaces::handle(ctx).update(ctx, |ws, ctx| {
                    if let Some((workspace, team_uid)) = ws.current_workspace().zip(team_uid)
                        && has_admin_permissions
                        && workspace
                            .settings
                            .addon_credits_settings
                            .auto_reload_enabled
                        && let Some(opt) = self
                            .addon_credits
                            .options
                            .get(self.addon_credits.selected_denomination)
                    {
                        ws.update_addon_credits_settings(
                            team_uid,
                            None,
                            None,
                            Some(opt.credits),
                            ctx,
                        );
                    }
                });
                ctx.notify();
            }
            BillingAndUsagePageAction::PurchaseAddonCredits { team_uid } => {
                if let Some(opt) = self
                    .addon_credits
                    .options
                    .get(self.addon_credits.selected_denomination)
                {
                    let credits = opt.credits;
                    let purchase_team_uid = *team_uid;
                    self.addon_credits.purchase_loading = true;
                    UserWorkspaces::handle(ctx).update(ctx, |ws, ctx| {
                        ws.purchase_addon_credits(purchase_team_uid, credits, ctx);
                    });
                    ctx.notify();
                }
            }
            BillingAndUsagePageAction::ShowAddOnCreditModal => {
                self.show_addon_credit_modal(ctx);
            }
            BillingAndUsagePageAction::UpdateAutoReloadEnabled { team_uid, enabled } => {
                let auto_reload_denomination_credits = if *enabled {
                    let Some(option) = self
                        .addon_credits
                        .options
                        .get(self.addon_credits.selected_denomination)
                    else {
                        self.show_toast(
                            "Unable to enable auto-reload until pricing options load.",
                            ToastFlavor::Error,
                            ctx,
                        );
                        return;
                    };
                    Some(option.credits)
                } else {
                    None
                };
                send_telemetry_from_ctx!(
                    TelemetryEvent::AutoReloadToggledFromBillingSettings {
                        enabled: *enabled,
                        banner_toggle_flag_enabled: FeatureFlag::BuildPlanAutoReloadBannerToggle
                            .is_enabled(),
                        post_purchase_modal_flag_enabled:
                            FeatureFlag::BuildPlanAutoReloadPostPurchaseModal.is_enabled(),
                    },
                    ctx
                );
                self.pending_auto_reload_toast = Some(if *enabled {
                    let credits = auto_reload_denomination_credits
                        .map(|c| c.separate_with_commas())
                        .unwrap_or_else(|| "your selected".to_string());
                    format!(
                        "Auto-reload enabled. We'll refill with {credits} credits when your balance runs low."
                    )
                } else {
                    "Auto-reload disabled.".to_string()
                });
                UserWorkspaces::handle(ctx).update(ctx, |ws, ctx| {
                    ws.update_addon_credits_settings(
                        *team_uid,
                        Some(*enabled),
                        None,
                        auto_reload_denomination_credits,
                        ctx,
                    );
                });
            }
        }
    }
}

fn should_show_open_admin_panel_link(
    is_team_admin: bool,
    is_workspace_admin: bool,
    is_enterprise_plan: bool,
) -> bool {
    (is_team_admin || is_workspace_admin) && is_enterprise_plan
}

#[cfg(test)]
#[path = "billing_and_usage_page_v2_tests.rs"]
mod tests;
