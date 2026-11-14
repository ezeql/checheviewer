//! CheCheViewer — visor de imágenes con foco en el teclado.
//!
//! La lógica vive acá y no en el binario para poder testearla sin abrir una
//! ventana.

#[macro_use]
pub mod diag;

pub mod app;
pub mod cache;
pub mod decode;
pub mod input;
pub mod loader;
pub mod model;
pub mod platform;
pub mod scan;
pub mod session;
pub mod ui;
pub mod view;
