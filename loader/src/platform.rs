#[cfg(feature="precursor")]
mod precursor;
#[cfg(feature="precursor")]
pub use precursor::*;

#[cfg(any(feature="cramium-soc", feature="cramium-fpga"))]
mod cramium;
#[cfg(any(feature="cramium-soc", feature="cramium-fpga"))]
pub use cramium::*;