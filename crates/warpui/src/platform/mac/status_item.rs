use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSSize, NSString};
use warpui_core::platform::menu::{CustomMenuItem, Menu, MenuItem, MenuItemPropertyChanges};
use warpui_core::platform::{StatusItem, StatusItemEntry};

use super::menus::make_dock_menu;

const STATUS_ITEM_ICON_POINTS: f64 = 18.0;

thread_local! {
    static INSTALLED_STATUS_ITEM: RefCell<Option<Retained<NSStatusItem>>> =
        const { RefCell::new(None) };
}

pub(super) fn set_status_item(status_item: Option<StatusItem>) {
    dispatch::Queue::main().exec_async(move || {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        match status_item {
            Some(status_item) => install_or_update(status_item, mtm),
            None => remove_installed_status_item(),
        }
    });
}

fn remove_installed_status_item() {
    if let Some(item) = INSTALLED_STATUS_ITEM.with(|slot| slot.borrow_mut().take()) {
        NSStatusBar::systemStatusBar().removeStatusItem(&item);
    }
}

fn install_or_update(status_item: StatusItem, mtm: MainThreadMarker) {
    let installed = INSTALLED_STATUS_ITEM.with(|slot| slot.borrow().clone());
    let item = installed.unwrap_or_else(|| {
        let item = create_status_item(&status_item, mtm);
        INSTALLED_STATUS_ITEM.with(|slot| *slot.borrow_mut() = Some(item.clone()));
        item
    });
    apply_status_item(&item, status_item, mtm);
}

fn create_status_item(status_item: &StatusItem, mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        match status_item_image(&status_item.icon_png) {
            Some(image) => button.setImage(Some(&image)),
            None => button.setTitle(&NSString::from_str(&status_item.tooltip)),
        }
    }
    item
}

fn apply_status_item(item: &NSStatusItem, status_item: StatusItem, mtm: MainThreadMarker) {
    if let Some(button) = item.button(mtm) {
        button.setToolTip(Some(&NSString::from_str(&status_item.tooltip)));
    }
    let menu = Menu::new(
        status_item.tooltip,
        status_item.entries.into_iter().map(menu_item).collect(),
    );
    let nsmenu = unsafe { make_dock_menu(menu) };
    item.setMenu(Some(&nsmenu));
}

fn status_item_image(png: &[u8]) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(png);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setSize(NSSize::new(
        STATUS_ITEM_ICON_POINTS,
        STATUS_ITEM_ICON_POINTS,
    ));
    Some(image)
}

fn menu_item(entry: StatusItemEntry) -> MenuItem {
    match entry {
        StatusItemEntry::Action {
            label,
            action,
            argument,
        } => MenuItem::Custom(CustomMenuItem::new(
            &label,
            move |ctx| ctx.status_item_action_triggered(action, argument.clone()),
            |_, _| MenuItemPropertyChanges::default(),
            None,
        )),
        StatusItemEntry::Separator => MenuItem::Separator,
    }
}
