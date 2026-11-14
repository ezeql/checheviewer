//! Abrir archivos desde el Finder.
//!
//! Cuando se hace doble clic en una imagen, o se usa "Abrir con", macOS **no**
//! pasa la ruta por `argv`: manda un Apple Event `kAEOpenDocuments`. Una app
//! que solo mire sus argumentos arranca vacía, como si la hubieran abierto
//! desde el Dock.
//!
//! Acá se instala un handler para ese evento antes de arrancar la ventana. Las
//! rutas que llegan se guardan en una cola que la app vacía en el próximo
//! frame.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};

use block2::RcBlock;
use objc2_core_foundation::{CFStringBuiltInEncodings, CFURL};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};

/// Rutas recibidas del sistema y todavía no consumidas.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Para sacar a la interfaz de su siesta cuando llega un archivo: sin esto,
/// egui puede estar esperando un evento de mouse y no redibujar.
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

// Códigos de cuatro letras de la API de Apple Events.
const K_CORE_EVENT_CLASS: u32 = four_cc(b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = four_cc(b"odoc");
const KEY_DIRECT_OBJECT: u32 = four_cc(b"----");
const TYPE_AE_LIST: u32 = four_cc(b"list");
const TYPE_FILE_URL: u32 = four_cc(b"furl");
const NO_ERR: i16 = 0;

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

impl AEDesc {
    const fn null() -> Self {
        Self { descriptor_type: 0, data_handle: std::ptr::null_mut() }
    }
}

type AEEventHandler =
    extern "C-unwind" fn(event: *const AEDesc, reply: *mut AEDesc, refcon: isize) -> i16;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C-unwind" {
    fn AEInstallEventHandler(
        event_class: u32,
        event_id: u32,
        handler: AEEventHandler,
        refcon: isize,
        is_sys_handler: u8,
    ) -> i16;

    fn AEGetParamDesc(event: *const AEDesc, key: u32, desired: u32, result: *mut AEDesc) -> i16;

    fn AECountItems(list: *const AEDesc, count: *mut isize) -> i16;

    #[allow(clippy::too_many_arguments)]
    fn AEGetNthPtr(
        list: *const AEDesc,
        index: isize,
        desired_type: u32,
        key: *mut u32,
        type_code: *mut u32,
        data: *mut c_void,
        maximum_size: isize,
        actual_size: *mut isize,
    ) -> i16;

    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
}

/// Instala el handler. Hay que llamarla antes de arrancar la ventana, para no
/// perder el evento con el que el sistema abre la app.
pub fn install(ctx_for_repaint: Option<egui::Context>) {
    if let Some(ctx) = ctx_for_repaint {
        let _ = REPAINT.set(ctx);
    }

    // SAFETY: la firma coincide con la que espera Carbon y el handler es una
    // función estática que vive toda la ejecución del programa.
    let status =
        unsafe { AEInstallEventHandler(K_CORE_EVENT_CLASS, K_AE_OPEN_DOCUMENTS, on_open, 0, 0) };

    if status != NO_ERR {
        // No es fatal: la app sigue andando, solo que abrirla desde el Finder
        // no traerá el archivo.
        eprintln!("checheviewer: no se pudo escuchar aperturas del Finder (error {status})");
    }
}

/// Vuelve a instalar el handler apenas AppKit termina de registrar los suyos.
///
/// Este es el punto delicado de todo el módulo: `NSApplication` registra sus
/// propios handlers de Apple Events mientras arranca y pisa el nuestro, así
/// que instalarlo solo desde `main` no alcanza. La notificación
/// `willFinishLaunching` llega después de ese registro y antes de que se
/// procese el evento con el que el Finder abrió la app, que es exactamente la
/// ventana que necesitamos para no perder el primer archivo.
pub fn install_on_launch() {
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        install(None);
    });

    // SAFETY: el bloque no captura nada prestado y el observador se mantiene
    // vivo mientras dure el programa.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(&name),
            None,
            None,
            &block,
        )
    };

    // El centro de notificaciones no retiene ni al observador ni al bloque:
    // hay que mantenerlos vivos a mano.
    std::mem::forget(observer);
    std::mem::forget(block);
}

/// Registra el contexto de egui una vez que existe, para poder despertar la
/// interfaz cuando llegue un archivo.
pub fn set_repaint_context(ctx: egui::Context) {
    let _ = REPAINT.set(ctx);
}

/// Saca la última ruta recibida, si hay alguna.
///
/// Si el usuario seleccionó varias imágenes se queda con una sola: la app
/// muestra la carpeta que la contiene, así que las demás igual van a estar a
/// un par de flechas de distancia.
pub fn take_pending() -> Option<PathBuf> {
    PENDING.lock().ok()?.pop()
}

extern "C-unwind" fn on_open(event: *const AEDesc, _reply: *mut AEDesc, _refcon: isize) -> i16 {
    // SAFETY: el sistema garantiza un evento válido durante la llamada.
    let paths = unsafe { paths_from_event(event) };

    if !paths.is_empty() {
        if let Ok(mut pending) = PENDING.lock() {
            pending.extend(paths);
        }
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    }
    NO_ERR
}

/// Extrae las rutas de la lista de documentos del evento.
///
/// # Safety
///
/// `event` tiene que ser un Apple Event válido.
unsafe fn paths_from_event(event: *const AEDesc) -> Vec<PathBuf> {
    let mut list = AEDesc::null();
    if unsafe { AEGetParamDesc(event, KEY_DIRECT_OBJECT, TYPE_AE_LIST, &mut list) } != NO_ERR {
        return Vec::new();
    }

    let mut count: isize = 0;
    let mut paths = Vec::new();
    if unsafe { AECountItems(&list, &mut count) } == NO_ERR {
        for index in 1..=count {
            if let Some(path) = unsafe { nth_path(&list, index) } {
                paths.push(path);
            }
        }
    }

    unsafe { AEDisposeDesc(&mut list) };
    paths
}

/// # Safety
///
/// `list` tiene que ser una lista de Apple Events válida e `index` estar en
/// rango.
unsafe fn nth_path(list: *const AEDesc, index: isize) -> Option<PathBuf> {
    // Las URLs de archivo son cortas; PATH_MAX de sobra alcanza incluso con el
    // escapado porcentual.
    let mut buffer = [0u8; 4096];
    let mut actual: isize = 0;

    let status = unsafe {
        AEGetNthPtr(
            list,
            index,
            TYPE_FILE_URL,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as isize,
            &mut actual,
        )
    };
    if status != NO_ERR || actual <= 0 {
        return None;
    }

    // Llega una URL `file://` con escapado porcentual; CoreFoundation la
    // convierte a una ruta del sistema de archivos sin que tengamos que
    // desescaparla a mano.
    let length = actual.min(buffer.len() as isize);
    let url = unsafe {
        CFURL::with_bytes(
            None,
            buffer.as_ptr(),
            length,
            CFStringBuiltInEncodings::EncodingUTF8.0,
            None,
        )
    }?;

    let mut path = [0u8; 4096];
    if !unsafe { url.file_system_representation(true, path.as_mut_ptr(), path.len() as isize) } {
        return None;
    }

    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    if end == 0 {
        return None;
    }

    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&path[..end]).to_os_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_codigos_de_cuatro_letras_coinciden_con_los_del_sistema() {
        assert_eq!(K_CORE_EVENT_CLASS, 0x6165_7674); // 'aevt'
        assert_eq!(K_AE_OPEN_DOCUMENTS, 0x6F64_6F63); // 'odoc'
        assert_eq!(KEY_DIRECT_OBJECT, 0x2D2D_2D2D); // '----'
        assert_eq!(TYPE_AE_LIST, 0x6C69_7374); // 'list'
        assert_eq!(TYPE_FILE_URL, 0x6675_726C); // 'furl'
    }

    #[test]
    fn instalar_el_handler_no_falla() {
        install(None);
        // Instalarlo dos veces tampoco: reemplaza al anterior.
        install(None);
    }

    #[test]
    fn sin_archivos_pendientes_no_devuelve_nada() {
        while take_pending().is_some() {}
        assert!(take_pending().is_none());
    }
}
