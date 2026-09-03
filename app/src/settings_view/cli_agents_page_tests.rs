use warpui::App;

use super::CLIAgentsPageView;
use crate::settings_view::settings_page::{FilteredPageType, PageType};

fn visible_widget_count(page: &PageType<CLIAgentsPageView>) -> usize {
    let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
        panic!("expected Uncategorized page");
    };
    widgets.len()
}

#[test]
fn approval_mode_query_isolates_its_own_widget() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = CLIAgentsPageView::build_page();
            let total = visible_widget_count(&page);
            assert!(total > 1, "the page needs more than one widget to isolate");

            let match_data = page.update_filter("yolo", ctx);
            assert!(match_data.is_truthy());
            assert_eq!(
                visible_widget_count(&page),
                1,
                "a term unique to the approval mode widget leaves only that widget"
            );

            page.update_filter("", ctx);
            assert_eq!(
                visible_widget_count(&page),
                total,
                "clearing the query restores every widget"
            );
        });
    });
}
