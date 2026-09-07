use qrcode::QrCode;
use qrcode::render::svg;
use remote_control::hosts::RemoteEndpoint;

const MIN_SIDE_PX: u32 = 220;

pub(crate) fn pairing_svg(endpoint: &RemoteEndpoint, token: &str) -> Option<String> {
    render_svg(&endpoint.pairing_url(token))
}

pub(crate) fn render_svg(value: &str) -> Option<String> {
    let code = QrCode::new(value.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(MIN_SIDE_PX, MIN_SIDE_PX)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

#[cfg(test)]
#[path = "qr_tests.rs"]
mod tests;
