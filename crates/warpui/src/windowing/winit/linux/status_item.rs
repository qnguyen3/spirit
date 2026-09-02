use image::ImageFormat;
use image::imageops::FilterType;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, ToolTip, Tray};
use warpui_core::platform::{StatusItem, StatusItemEntry};
use winit::event_loop::EventLoopProxy;

use crate::windowing::winit::app::CustomEvent;

const ICON_SIZES: [u32; 2] = [22, 48];

struct StatusItemTray {
    id: String,
    status_item: StatusItem,
    icons: Vec<Icon>,
    proxy: EventLoopProxy<CustomEvent>,
}

impl StatusItemTray {
    fn trigger(&self, action: &'static str, argument: String) {
        let _ = self
            .proxy
            .send_event(CustomEvent::StatusItemActionTriggered(action, argument));
    }
}

impl Tray for StatusItemTray {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn title(&self) -> String {
        self.status_item.tooltip.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: self.status_item.tooltip.clone(),
            ..Default::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if let Some((action, argument)) = self.status_item.primary_action() {
            self.trigger(action, argument);
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        self.status_item
            .entries
            .iter()
            .map(|entry| match entry {
                StatusItemEntry::Action {
                    label,
                    action,
                    argument,
                } => {
                    let action = *action;
                    let argument = argument.clone();
                    StandardItem {
                        label: label.clone(),
                        activate: Box::new(move |tray: &mut Self| {
                            tray.trigger(action, argument.clone())
                        }),
                        ..Default::default()
                    }
                    .into()
                }
                StatusItemEntry::Separator => MenuItem::Separator,
            })
            .collect()
    }
}

pub struct StatusItemHandle(Handle<StatusItemTray>);

impl StatusItemHandle {
    pub fn update(&mut self, status_item: StatusItem) {
        let _ = self.0.update(|tray| tray.status_item = status_item);
    }
}

impl Drop for StatusItemHandle {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

pub fn install(
    status_item: StatusItem,
    app_id: &str,
    proxy: EventLoopProxy<CustomEvent>,
) -> Option<StatusItemHandle> {
    let icons = tray_icons(&status_item.icon_png);
    let tray = StatusItemTray {
        id: app_id.to_owned(),
        status_item,
        icons,
        proxy,
    };
    match tray.spawn() {
        Ok(handle) => Some(StatusItemHandle(handle)),
        Err(err) => {
            log::warn!("Unable to register the status notifier item: {err}");
            None
        }
    }
}

fn tray_icons(png: &[u8]) -> Vec<Icon> {
    let Ok(image) = image::load_from_memory_with_format(png, ImageFormat::Png) else {
        return Vec::new();
    };
    ICON_SIZES
        .iter()
        .map(|&size| {
            let mut data = image
                .resize_exact(size, size, FilterType::Lanczos3)
                .to_rgba8()
                .into_raw();
            for pixel in data.chunks_exact_mut(4) {
                pixel.rotate_right(1);
            }
            Icon {
                width: size as i32,
                height: size as i32,
                data,
            }
        })
        .collect()
}
