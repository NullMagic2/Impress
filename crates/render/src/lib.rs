//! Vector scene graph and presentation geometry for Impress.
//!
//! The key rule is that text stays vector throughout Impress's own pipeline.
//! We shape text into glyph IDs and fractional positions, extract outline
//! geometry from the font, and keep the page as a vector display list.
//! The wxDragon canvas paints this scene; the scene itself is not a bitmap cache.

mod color;
mod geometry;
mod glyphs;
mod scene;
mod search_marks;
pub use search_marks::search_marks;
mod raster;
pub use raster::*;
mod viewport;
mod overlay;
mod navigation;

pub use color::*;
pub use geometry::*;
pub use glyphs::*;
pub use scene::*;

pub use viewport::*;
pub use overlay::*;
pub use navigation::*;



pub mod picture_shapes;
