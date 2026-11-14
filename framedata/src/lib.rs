//! The reading step: a PresentMon per-frame file becomes a table of numbers (`frames`), and that
//! table is packed small for sending (`pack`, format FPSF1) and unpacked again. Nothing here judges
//! a recording. The recorder uses it to pack; whatever judges a recording uses it to unpack.
//! `net` does the same for the network timing recorded beside the frames (format FPSN1).

pub mod frames;
pub mod net;
pub mod pack;
