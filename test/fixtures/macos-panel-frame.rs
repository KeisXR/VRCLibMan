// Compile with rustc; the native Swift test calls the production geometry helper.
#[path = "../../src-tauri/src/browser_surface/macos_geometry.rs"]
mod geometry;
use geometry::{Frame, panel_frame};

fn main() {
    let values = std::env::args().skip(1).map(|s| s.parse::<f64>().unwrap()).collect::<Vec<_>>();
    assert_eq!(values.len(), 9);
    let frame = |i| Frame { x: values[i], y: values[i+1], width: values[i+2], height: values[i+3] };
    let result = panel_frame(frame(0), frame(4), values[8] != 0.);
    println!("{},{},{},{}", result.x, result.y, result.width, result.height);
}
