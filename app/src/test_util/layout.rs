use pathfinder_geometry::vector::vec2f;
use warpui::platform::WindowStyle;
use warpui::{App, EntityIdSet, Presenter, TypedActionView, View, ViewContext, WindowInvalidation};

const WINDOW_WIDTH: f32 = 1200.;
const WINDOW_HEIGHT: f32 = 800.;

pub fn build_scene_for_root_view<T, F>(app: &mut App, build_root_view: F)
where
    T: View + TypedActionView,
    F: FnOnce(&mut ViewContext<T>) -> T,
{
    let (window_id, _view) = app.add_window(WindowStyle::NotStealFocus, build_root_view);
    let view_id = app.root_view_id(window_id).expect("window has a root view");
    let mut presenter = Presenter::new(window_id);

    app.update(|ctx| {
        let mut updated = EntityIdSet::default();
        updated.insert(view_id);
        presenter.invalidate(
            WindowInvalidation {
                updated,
                ..Default::default()
            },
            ctx,
        );
        presenter.build_scene(vec2f(WINDOW_WIDTH, WINDOW_HEIGHT), 1., None, ctx);
    });
}
