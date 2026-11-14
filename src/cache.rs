use std::path::{Path, PathBuf};

use lru::LruCache;

/// Presupuesto por defecto del cache de imágenes.
///
/// Se mide en bytes de píxeles, no en cantidad de imágenes: diez fotos de
/// 50 MP y mil miniaturas ocupan cosas muy distintas, y lo que importa es la
/// memoria, no el conteo.
pub const DEFAULT_BUDGET: usize = 1024 * 1024 * 1024; // 1 GiB

/// Presupuesto del cache de miniaturas.
///
/// Una miniatura de 256×256 RGBA son 256 KiB, así que esto alcanza para unas
/// mil celdas: bastante más de lo que entra en pantalla, y se recorre una
/// carpeta grande sin volver a decodificar lo ya visto.
pub const THUMBNAIL_BUDGET: usize = 256 * 1024 * 1024; // 256 MiB

/// Cache LRU acotado por bytes.
pub struct ByteCache<V> {
    entries: LruCache<PathBuf, (V, usize)>,
    bytes: usize,
    budget: usize,
}

impl<V> ByteCache<V> {
    pub fn new(budget: usize) -> Self {
        Self { entries: LruCache::unbounded(), bytes: 0, budget }
    }

    /// Devuelve el valor y lo marca como recién usado.
    pub fn get(&mut self, path: &Path) -> Option<&V> {
        self.entries.get(path).map(|(value, _)| value)
    }

    /// ¿Está en el cache? No altera el orden de uso.
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.peek(path).is_some()
    }

    /// Guarda un valor de `size` bytes y desaloja lo más viejo si hace falta.
    pub fn insert(&mut self, path: PathBuf, value: V, size: usize) {
        if let Some((_, previous)) = self.entries.put(path, (value, size)) {
            self.bytes -= previous;
        }
        self.bytes += size;
        self.evict();
    }

    fn evict(&mut self) {
        // Nunca se desaloja la última entrada: es la que el usuario está
        // mirando, y una sola foto puede superar el presupuesto por sí sola.
        while self.bytes > self.budget && self.entries.len() > 1 {
            match self.entries.pop_lru() {
                Some((_, (_, size))) => self.bytes -= size,
                None => break,
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn guarda_y_devuelve() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 10);
        assert_eq!(cache.get(&path("a")), Some(&1));
        assert_eq!(cache.bytes(), 10);
        assert!(cache.contains(&path("a")));
        assert!(!cache.contains(&path("b")));
    }

    #[test]
    fn desaloja_lo_mas_viejo_al_pasarse_del_presupuesto() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 50);
        cache.insert(path("b"), 2, 50);
        cache.insert(path("c"), 3, 50); // se pasa: se va "a"

        assert!(!cache.contains(&path("a")));
        assert!(cache.contains(&path("b")));
        assert!(cache.contains(&path("c")));
        assert_eq!(cache.bytes(), 100);
    }

    #[test]
    fn usar_una_entrada_la_salva_del_desalojo() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 50);
        cache.insert(path("b"), 2, 50);
        cache.get(&path("a")); // "a" pasa a ser la más reciente
        cache.insert(path("c"), 3, 50);

        assert!(cache.contains(&path("a")), "la entrada recién usada no debería desalojarse");
        assert!(!cache.contains(&path("b")));
    }

    #[test]
    fn nunca_desaloja_la_unica_entrada_aunque_no_entre() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("gigante"), 1, 5_000);

        assert!(cache.contains(&path("gigante")), "la imagen actual tiene que sobrevivir");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn reemplazar_una_clave_no_duplica_los_bytes() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        cache.insert(path("a"), 1, 100);
        cache.insert(path("a"), 2, 30);

        assert_eq!(cache.bytes(), 30);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&path("a")), Some(&2));
    }

    #[test]
    fn el_uso_de_memoria_no_crece_indefinidamente() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        for i in 0..500 {
            cache.insert(path(&format!("img{i}")), i, 100);
            assert!(cache.bytes() <= 1_000, "el cache se pasó del presupuesto en la vuelta {i}");
        }
        assert_eq!(cache.len(), 10);
    }

    #[test]
    fn clear_libera_todo() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        cache.insert(path("a"), 1, 100);
        cache.clear();

        assert_eq!(cache.bytes(), 0);
        assert!(cache.is_empty());
    }
}
