use warpui_core::{App, ModelHandle};

use crate::model::{OnboardingStateModel, OnboardingStep};

fn add_test_model(app: &mut App) -> ModelHandle<OnboardingStateModel> {
    app.add_model(|_| OnboardingStateModel::new())
}

fn step(app: &App, model: &ModelHandle<OnboardingStateModel>) -> OnboardingStep {
    model.read(app, |model, _| model.step())
}

#[test]
fn terminal_path_skips_agent_slides() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        // Onboarding goes Intro → Customize → ThemePicker; the intention, AI setup
        // and third-party slides are no longer part of the flow.
        for expected in [OnboardingStep::Customize, OnboardingStep::ThemePicker] {
            model.update(&mut app, |model, ctx| model.next(ctx));
            assert_eq!(step(&app, &model), expected);
        }

        // Back navigation mirrors the forward path.
        model.update(&mut app, |model, ctx| model.back(ctx));
        assert_eq!(step(&app, &model), OnboardingStep::Customize);
        model.update(&mut app, |model, ctx| model.back(ctx));
        assert_eq!(step(&app, &model), OnboardingStep::Intro);
    });
}

#[test]
fn the_theme_picker_is_the_last_step_and_completing_it_does_not_advance() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);

        model.update(&mut app, |model, ctx| {
            model.set_step(OnboardingStep::ThemePicker, ctx);
            model.next(ctx);
        });
        assert_eq!(step(&app, &model), OnboardingStep::ThemePicker);
    });
}

#[test]
fn progress_reports_terminal_path_uses_three_dot_variant() {
    App::test((), |mut app| async move {
        let model = add_test_model(&mut app);
        let cases = [
            (OnboardingStep::Intro, (0, 3)),
            (OnboardingStep::Customize, (1, 3)),
            (OnboardingStep::ThemePicker, (2, 3)),
        ];
        for (target, expected) in cases {
            model.update(&mut app, |model, ctx| model.set_step(target, ctx));
            let progress = model.read(&app, |model, _| model.progress());
            assert_eq!(progress, expected, "unexpected dots for {target:?}");
        }
    });
}
