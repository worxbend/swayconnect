use super::*;

const SQUARE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="256"
    viewBox="0 0 512 256"><rect width="512" height="256" fill="#3a7"/></svg>"##;

fn png_of(width: u32, height: u32) -> Vec<u8> {
    Pixmap::new(width, height)
        .expect("pixmap")
        .encode_png()
        .expect("encode")
}

fn rendered_size(source: &[u8]) -> (u32, u32) {
    let output = render(source).expect("render");
    assert!(png::is_servable(&output));
    png::declared_size(&output).expect("size")
}

#[test]
fn svg_fits_into_128_px_keeping_its_aspect() {
    assert_eq!(rendered_size(SQUARE_SVG.as_bytes()), (128, 64));
}

#[test]
fn a_small_svg_is_drawn_at_128_px() {
    let small = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="8"/></svg>"#;
    assert_eq!(rendered_size(small.as_bytes()), (128, 128));
}

#[test]
fn png_is_shrunk_to_128_px_and_small_png_keeps_its_size() {
    assert_eq!(rendered_size(&png_of(256, 512)), (64, 128));
    assert_eq!(rendered_size(&png_of(48, 48)), (48, 48));
}

#[test]
fn a_png_wider_than_1024_px_is_refused_by_its_header() {
    let mut huge = png::header(MAX_SOURCE_SIDE_PX + 1, 16);
    huge.extend_from_slice(&[0; 64]);
    assert!(matches!(
        render(&huge),
        Err(RasterError::SourceTooWide(1025, 16))
    ));
    assert!(render(&png_of(1024, 1)).is_ok());
}

#[test]
fn svg_filters_are_refused() {
    let blur = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
        <filter id="f"><feGaussianBlur stdDeviation="9"/></filter>
        <rect width="16" height="16" filter="url(#f)"/></svg>"#;
    assert!(matches!(
        render(blur.as_bytes()),
        Err(RasterError::SvgFilters)
    ));
}

#[test]
fn svg_with_too_many_nodes_is_refused() {
    let svg = |count: usize| {
        let rects = r#"<rect x="1" y="1" width="2" height="2"/>"#.repeat(count);
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">{rects}</svg>"#)
    };
    assert!(render(svg(MAX_SVG_NODES).as_bytes()).is_ok());
    assert!(matches!(
        render(svg(MAX_SVG_NODES + 1).as_bytes()),
        Err(RasterError::SvgTooManyNodes)
    ));
}

#[test]
fn garbage_and_empty_images_fail() {
    assert!(matches!(render(b"not an image"), Err(RasterError::Svg(_))));
    let empty = r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#;
    assert!(render(empty.as_bytes()).is_err());
}
