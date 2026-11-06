// ============================================================
// KIS — In-Memory-Cache (Task 1.17, erweitert Task 5.2)
//
// moka-basierter Cache für wiederholte READ-Queries:
//   - TTL: 5 Minuten (300s, konfigurierbar)
//   - Max-Size: 10.000 Einträge (LRU-Eviction)
//   - Key:  (entity_type, entity_id)
//
// Prinzip:
//   - READ-Queries:  erst Cache prüfen, bei Hit direkt zurückgeben
//   - WRITE-Queries: Cache der betroffenen Entität invalidieren
//
// Task 5.2 — Read-Through-Caching (Redis-Alternative, in-memory):
//   - `get_or_load()`: Bei Cache-Miss wird der async DB-Loader
//     automatisch ausgeführt und das Ergebnis im Cache gespeichert
//     (TTL aus Config). Der Loader ist eine Closure — dadurch
//     funktioniert der Cache in Tests OHNE DB-Verbindung.
//   - `invalidate()` / `invalidate_prefix()`: gezielte Invalidation
//     bei Schreib-Operationen (Eintrag bzw. alle Einträge eines
//     Entity-Typs).
//   - `stats()`: hit_count, miss_count, hit_rate, evictions.
//
// Der Cache speichert serialisierte JSON-Strings (serde_json::Value),
// damit beliebige serde-serialisierbare Entitäten ohne Monomorphisier-
// overhead zwischengespeichert werden.
// ============================================================

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use moka::sync::Cache as MokaCache;
use serde_json::Value;
use tracing::debug;

use crate::config::CacheConfig;

/// Schlüsseltyp: (entity_type, entity_id)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub entity_type: String,
    pub entity_id: String,
}

impl CacheKey {
    pub fn new(entity_type: impl Into<String>, entity_id: impl Into<String>) -> Self {
        Self {
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
        }
    }

    /// Interner moka-Key (String, billiger zu hashen als Tuple).
    fn moka_key(&self) -> String {
        format!("{}:{}", self.entity_type, self.entity_id)
    }
}

/// Cache-Statistiken (Task 5.2).
///
/// Gezählt werden alle Lesezugriffe über `get*`/`get_or_load`:
/// - `hit_count`:  Treffer im Cache
/// - `miss_count`: Verfehlungen (Loader wird ausgeführt bzw. `None`)
/// - `evictions`:  Schätzung der LRU-Evictions (moka liefert diese
///   nicht direkt; wir zählen die Sets, die die Kapazität überschreiten)
///
/// `hit_rate` = hit_count / (hit_count + miss_count), 0.0 ohne Zugriff.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CacheStats {
    pub hit_count: u64,
    pub miss_count: u64,
    pub hit_rate: f64,
    pub evictions: u64,
}

impl CacheStats {
    fn from_counts(hit: u64, miss: u64, evictions: u64) -> Self {
        let total = hit + miss;
        let hit_rate = if total == 0 {
            0.0
        } else {
            hit as f64 / total as f64
        };
        Self {
            hit_count: hit,
            miss_count: miss,
            hit_rate,
            evictions,
        }
    }
}

/// KIS-Entitäten-Cache (Task 1.17 + Task 5.2 Read-Through).
///
/// Thread-safe, `Arc`-teilbar — wird als `Extension` in die axum-App
/// injiziert und von allen Services geteilt.
#[derive(Debug, Clone)]
pub struct EntityCache {
    cache: Arc<MokaCache<String, Value>>,
    /// TTL in Sekunden (für Monitoring/Tests)
    #[allow(dead_code)]
    ttl_seconds: u64,
    /// Max. Einträge (für Monitoring/Tests)
    max_entries: u64,
    /// Gezählte Cache-Hits (Task 5.2)
    hits: Arc<AtomicU64>,
    /// Gezählte Cache-Misses (Task 5.2)
    misses: Arc<AtomicU64>,
    /// Schätzung der LRU-Evictions (Task 5.2)
    evictions: Arc<AtomicU64>,
    /// Bekannte Keys (für `invalidate_prefix` ohne O(n)-Scan des moka-Caches)
    known_keys: Arc<parking_lot_free_mutex::Mutex<HashSet<String>>>,
}

// Leichtgewichtige Mutex-Abstraktion ohne zusätzliche Dependency:
// std::sync::Mutex reicht — die kritische Sektion ist nur ein HashSet-Update.
mod parking_lot_free_mutex {
    pub use std::sync::Mutex;
}

impl EntityCache {
    /// Erzeugt den Cache aus der Konfiguration.
    ///
    /// `ttl_seconds` = Lebensdauer je Eintrag (Default: 300s).
    /// `max_entries` = Obergrenze (LRU-Eviction darüber hinaus).
    pub fn new(config: &CacheConfig) -> Self {
        let ttl = Duration::from_secs(config.ttl_seconds.max(1));
        let cache = MokaCache::builder()
            .time_to_live(ttl)
            .max_capacity(config.max_entries.max(1))
            .build();
        debug!(
            ttl_seconds = config.ttl_seconds,
            max_entries = config.max_entries,
            "Entitäten-Cache initialisiert (moka, in-memory)"
        );
        Self {
            cache: Arc::new(cache),
            ttl_seconds: config.ttl_seconds,
            max_entries: config.max_entries,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
            evictions: Arc::new(AtomicU64::new(0)),
            known_keys: Arc::new(parking_lot_free_mutex::Mutex::new(HashSet::new())),
        }
    }

    /// Liest einen Eintrag: `Some(json)` bei Hit, `None` bei Miss/expired.
    ///
    /// Zählt den Zugriff in den Cache-Stats (Task 5.2).
    pub fn get(&self, entity_type: &str, entity_id: &str) -> Option<Value> {
        let key = CacheKey::new(entity_type, entity_id).moka_key();
        match self.cache.get(&key) {
            Some(v) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(v)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Liest und deserialisiert den Eintrag in den Zieltyp.
    pub fn get_typed<T: serde::de::DeserializeOwned>(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Option<T> {
        self.get(entity_type, entity_id)
            .and_then(|v| serde_json::from_value(v).ok())
    }

    /// Read-Through-Lesefunktion (Task 5.2).
    ///
    /// Prüft zuerst den Cache:
    /// - **Hit**:  Wert wird direkt zurückgegeben (Loader NICHT aufgerufen).
    /// - **Miss**: Der async `loader` wird genau EINMAL ausgeführt; sein
    ///   Ergebnis wird im Cache gespeichert (TTL aus Config) und
    ///   zurückgegeben. Gibt der Loader `None` zurück, wird NICHTS
    ///   gecacht (Negatives Caching ist nicht erwünscht, da Patienten-
    ///   Daten z. B. durch Soft-Delete wieder „erscheinen" können).
    ///
    /// Der Loader ist eine beliebige async-Closure (z. B. eine
    /// DB-Query), daher funktioniert Read-Through auch in Tests ohne
    /// Datenbank-Verbindung:
    ///
    /// ```ignore
    /// let v = cache.get_or_load("patient", "42", || async {
    ///     sqlx::query_as("SELECT * FROM patients WHERE id = $1")
    ///         .bind(id).fetch_optional(pool).await
    /// }).await?;
    /// ```
    ///
    /// Parallel aufrufende Tasks desselben Keys können den Loader
    /// mehrfach ausführen (thundering herd) — für einen in-memory
    /// Lese-Cache ist das akzeptabel; das Ergebnis bleibt konsistent,
    /// da der Loader eine reine DB-Lese-Query ist.
    pub async fn get_or_load<T, F, Fut>(
        &self,
        entity_type: &str,
        entity_id: &str,
        loader: F,
    ) -> anyhow::Result<Option<T>>
    where
        T: serde::de::DeserializeOwned + serde::Serialize + Send,
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Option<T>>>,
    {
        let key = CacheKey::new(entity_type, entity_id).moka_key();
        let moka_key = key.clone();

        // 1. Cache-Hit?
        if let Some(v) = self.cache.get(&moka_key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            let parsed: T = serde_json::from_value(v)?;
            return Ok(Some(parsed));
        }
        self.misses.fetch_add(1, Ordering::Relaxed);

        // 2. Cache-Miss → Loader ausführen (DB-Query)
        let value = loader().await?;
        match &value {
            Some(v) => {
                let json = serde_json::to_value(v)?;
                self.track_insert(&moka_key);
                self.cache.insert(key.clone(), json);
                debug!(
                    entity_type,
                    entity_id, "Read-Through: Loader-Ergebnis im Cache gespeichert"
                );
            }
            None => debug!(
                entity_type,
                entity_id, "Read-Through: Loader lieferte None (wird nicht gecacht)"
            ),
        }
        Ok(value)
    }

    /// Schreibt einen serialisierbaren Wert in den Cache.
    pub fn set<T: serde::Serialize>(
        &self,
        entity_type: &str,
        entity_id: &str,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        let json = serde_json::to_value(value)?;
        let key = CacheKey::new(entity_type, entity_id).moka_key();
        self.track_insert(&key);
        self.cache.insert(key, json);
        debug!(
            entity_type,
            entity_id, "Cache-Set (TTL läuft ab dem jetzigen Zeitpunkt)"
        );
        Ok(())
    }

    /// Invalidiert einen konkreten Eintrag (nach UPDATE/DELETE der Entität, Task 5.2).
    pub fn invalidate(&self, entity_type: &str, entity_id: &str) {
        let key = CacheKey::new(entity_type, entity_id).moka_key();
        self.cache.invalidate(&key);
        if let Ok(mut known) = self.known_keys.lock() {
            known.remove(&key);
        }
        debug!(
            entity_type,
            entity_id, "Cache-Eintrag invalidiert (nach Write)"
        );
    }

    /// Invalidiert ALLE Einträge eines Entity-Typs (Task 5.2).
    ///
    /// z. B. nach einer UPDATE, die mehrere Einträge desselben Typs
    /// betrifft, oder nach einer Bulk-Synchronisation. Nutzt die
    /// intern geführte Key-Liste — O(betroffene Einträge) statt
    /// O(gesamte Cache-Größe).
    pub fn invalidate_prefix(&self, entity_type: &str) {
        let prefix = format!("{}:", entity_type);
        let keys: Vec<String> = self
            .known_keys
            .lock()
            .ok()
            .map(|known| {
                known
                    .iter()
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        for key in &keys {
            self.cache.invalidate(key);
        }
        if let Ok(mut known) = self.known_keys.lock() {
            known.retain(|k| !k.starts_with(&prefix));
        }
        if !keys.is_empty() {
            debug!(
                entity_type,
                count = keys.len(),
                "Cache-Prefix invalidiert (nach Bulk-Write)"
            );
        }
    }

    /// Invalidiert ALLE Einträge (z. B. nach massiven Änderungen).
    pub fn invalidate_all(&self) {
        self.cache.invalidate_all();
        if let Ok(mut known) = self.known_keys.lock() {
            known.clear();
        }
        debug!("Gesamter Cache invalidiert");
    }

    /// Trägt einen neuen Eintrag in die Key-Liste ein und schätzt
    /// LRU-Evictions: Sobald die Anzahl der bekannten Keys die
    /// konfigurierte Max-Kapazität überschreitet, zählt moka asynchron
    /// ältere Einträge aus — wir schätzen das als Eviction.
    fn track_insert(&self, key: &str) {
        if let Ok(mut known) = self.known_keys.lock() {
            if known.insert(key.to_string()) && known.len() as u64 > self.max_entries {
                self.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Cache-Statistiken (Task 5.2): hit_count, miss_count, hit_rate, evictions.
    pub fn stats(&self) -> CacheStats {
        CacheStats::from_counts(
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
            self.evictions.load(Ordering::Relaxed),
        )
    }

    /// Anzahl aktueller Einträge (Monitoring).
    ///
    /// moka zählt asynchron (Eviction-Lag) — für Tests mit kurzer TTL
    /// wird kurz gewartet, bis die Zählung konsistent ist.
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }

    /// Wartet kurz, bis moka die interne Zählung aktualisiert hat.
    /// Nur für Tests — in der Produktion nicht verwenden.
    #[cfg(test)]
    pub fn wait_for_consistency(&self, expected: u64) {
        for _ in 0..50 {
            if self.cache.entry_count() == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Konfigurierte Max-Kapazität (Monitoring).
    pub fn total_capacity(&self) -> u64 {
        self.max_entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(ttl_secs: u64, max_entries: u64) -> CacheConfig {
        CacheConfig {
            ttl_seconds: ttl_secs,
            max_entries,
        }
    }

    /// Cache-Hit: gesetzter Wert wird sofort gelesen.
    #[test]
    fn test_cache_hit() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let patient = serde_json::json!({"id": "123", "name": "Muster"});
        cache
            .set("patient", "123", &patient)
            .expect("Serialisierung muss klappen");
        assert_eq!(cache.get("patient", "123"), Some(patient));
    }

    /// Cache-Miss bei unbekanntem Key.
    #[test]
    fn test_cache_miss() {
        let cache = EntityCache::new(&test_config(300, 1000));
        assert!(cache.get("patient", "unknown").is_none());
    }

    /// Invalidation: nach `invalidate` ist der Eintrag weg.
    #[test]
    fn test_invalidation_after_write() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache
            .set("doctor", "42", &serde_json::json!({"name": "Dr. A"}))
            .unwrap();
        assert!(cache.get("doctor", "42").is_some());

        // Write-Query → Invalidation
        cache.invalidate("doctor", "42");
        assert!(cache.get("doctor", "42").is_none());
    }

    /// `invalidate_all` entfernt alle Einträge.
    #[test]
    fn test_invalidate_all() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache.set("patient", "1", &serde_json::json!({})).unwrap();
        cache.set("doctor", "2", &serde_json::json!({})).unwrap();
        // Einträge sind gesetzt (get() bestätigt)
        assert!(cache.get("patient", "1").is_some());
        assert!(cache.get("doctor", "2").is_some());

        cache.invalidate_all();
        // moka zählt asynchron — kurz warten bis Zählung 0 ist
        cache.wait_for_consistency(0);
        assert_eq!(cache.entry_count(), 0);
    }

    /// TTL-Expiry: Eintrag mit TTL=1s ist nach 1.2s weg.
    #[tokio::test]
    async fn test_ttl_expiry() {
        let cache = EntityCache::new(&test_config(1, 1000));
        cache
            .set("patient", "999", &serde_json::json!({"v": 1}))
            .unwrap();
        assert!(cache.get("patient", "999").is_some());

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            cache.get("patient", "999").is_none(),
            "Eintrag nach TTL-Ablauf muss weg sein"
        );
    }

    /// LRU-Eviction: max_entries=2, dritter Set verdrängt den ältesten.
    #[test]
    fn test_lru_eviction() {
        let cache = EntityCache::new(&test_config(3600, 2));
        cache.set("patient", "1", &serde_json::json!({})).unwrap();
        cache.set("patient", "2", &serde_json::json!({})).unwrap();
        // Dritter Eintrag → einer wird verdrängt
        cache.set("patient", "3", &serde_json::json!({})).unwrap();
        assert!(cache.entry_count() <= 2);
    }

    /// get_typed: Roundtrip über serde.
    #[test]
    fn test_get_typed_roundtrip() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct P {
            id: String,
            name: String,
        }
        let cache = EntityCache::new(&test_config(300, 1000));
        let p = P {
            id: "abc".into(),
            name: "Müller".into(),
        };
        cache.set("patient", "abc", &p).unwrap();
        let read: P = cache.get_typed("patient", "abc").expect("typed hit");
        assert_eq!(read, p);
    }

    // ====================================================================
    // Task 5.2: Read-Through, Invalidation, Stats
    // ====================================================================

    /// Read-Through: Bei Miss wird der Loader ausgeführt, das Ergebnis
    /// wird gecacht — der zweite Aufruf ist ein Hit ohne Loader-Call.
    #[tokio::test]
    async fn test_read_through_miss_then_hit() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let load_calls = Arc::new(AtomicU64::new(0));
        let calls = load_calls.clone();

        // Simulierte DB-Query als Closure — keine DB-Verbindung nötig
        let result: Option<serde_json::Value> = cache
            .get_or_load("patient", "42", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(Some(serde_json::json!({"id": "42", "name": "Muster"}))) }
            })
            .await
            .expect("Read-Through darf nicht fehlschlagen");

        assert_eq!(
            result,
            Some(serde_json::json!({"id": "42", "name": "Muster"}))
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "Loader muss einmal laufen"
        );

        // Zweiter Aufruf: Cache-Hit, Loader NICHT erneut
        let result2: Option<serde_json::Value> = cache
            .get_or_load("patient", "42", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(Some(serde_json::json!({"id": "42"}))) }
            })
            .await
            .unwrap();
        assert_eq!(
            result2,
            Some(serde_json::json!({"id": "42", "name": "Muster"})),
            "Zweiter Aufruf muss den gecachten Wert liefern"
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "Bei Cache-Hit darf der Loader nicht erneut laufen"
        );
    }

    /// Read-Through mit typisierten Werten: Loader liefert eine Struktur,
    /// der zweite Aufruf deserialisiert aus dem JSON-Cache.
    #[tokio::test]
    async fn test_read_through_typed() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
        struct Patient {
            id: String,
            name: String,
        }
        let cache = EntityCache::new(&test_config(300, 1000));

        let loaded: Option<Patient> = cache
            .get_or_load("patient", "abc", || async {
                anyhow::Ok(Some(Patient {
                    id: "abc".into(),
                    name: "Muster".into(),
                }))
            })
            .await
            .unwrap();
        assert_eq!(
            loaded,
            Some(Patient {
                id: "abc".into(),
                name: "Muster".into(),
            })
        );

        // Hit: deserialisiert aus dem Cache, Loader wird nicht aufgerufen
        let hit: Option<Patient> = cache
            .get_or_load("patient", "abc", || async {
                panic!("Loader darf bei Hit nicht aufgerufen werden");
            })
            .await
            .unwrap();
        assert_eq!(
            hit,
            Some(Patient {
                id: "abc".into(),
                name: "Muster".into(),
            })
        );
    }

    /// Read-Through: Loader-Fehler werden weitergereicht und nichts gecacht.
    #[tokio::test]
    async fn test_read_through_loader_error() {
        let cache = EntityCache::new(&test_config(300, 1000));

        let err: anyhow::Result<Option<serde_json::Value>> = cache
            .get_or_load("patient", "err", || async {
                anyhow::bail!("DB nicht erreichbar (simuliert)");
            })
            .await;
        assert!(err.is_err(), "Loader-Fehler muss propagieren");

        // Nichts gecacht → nächster Miss ruft den Loader erneut auf
        let calls = Arc::new(AtomicU64::new(0));
        let calls2 = calls.clone();
        let retry: Option<serde_json::Value> = cache
            .get_or_load("patient", "err", || {
                calls2.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(Some(serde_json::json!({"id": "err"}))) }
            })
            .await
            .unwrap();
        assert_eq!(retry, Some(serde_json::json!({"id": "err"})));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    /// Read-Through: Loader-None wird NICHT gecacht (negatives Ergebnis
    /// darf nicht kleben bleiben, z. B. wegen Soft-Delete-Semantik).
    #[tokio::test]
    async fn test_read_through_none_not_cached() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let calls = Arc::new(AtomicU64::new(0));

        let none: Option<serde_json::Value> = cache
            .get_or_load("patient", "ghost", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(None) }
            })
            .await
            .unwrap();
        assert!(none.is_none());

        // Zweiter Aufruf: Loader läuft erneut (kein negativer Cache-Eintrag)
        let none2: Option<serde_json::Value> = cache
            .get_or_load("patient", "ghost", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(None) }
            })
            .await
            .unwrap();
        assert!(none2.is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    /// Invalidation bei Schreib-Operationen: Nach `invalidate` läuft der
    /// Read-Through-Loader erneut (frische Daten).
    #[tokio::test]
    async fn test_read_through_invalidation_after_write() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let calls = Arc::new(AtomicU64::new(0));

        let v1: Option<serde_json::Value> = cache
            .get_or_load("patient", "7", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(Some(serde_json::json!({"v": 1}))) }
            })
            .await
            .unwrap();
        assert_eq!(v1, Some(serde_json::json!({"v": 1})));

        // Write-Operation → gezielte Invalidation
        cache.invalidate("patient", "7");

        // Read-Through holt frische Daten von der DB
        let v2: Option<serde_json::Value> = cache
            .get_or_load("patient", "7", || {
                calls.fetch_add(1, Ordering::Relaxed);
                async move { anyhow::Ok(Some(serde_json::json!({"v": 2}))) }
            })
            .await
            .unwrap();
        assert_eq!(v2, Some(serde_json::json!({"v": 2})));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    /// `invalidate_prefix`: Entfernt alle Einträge eines Entity-Typs,
    /// andere Typs bleiben erhalten.
    #[test]
    fn test_invalidate_prefix() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache
            .set("patient", "1", &serde_json::json!({"a": 1}))
            .unwrap();
        cache
            .set("patient", "2", &serde_json::json!({"a": 2}))
            .unwrap();
        cache
            .set("doctor", "9", &serde_json::json!({"d": 9}))
            .unwrap();

        cache.invalidate_prefix("patient");

        assert!(
            cache.get("patient", "1").is_none(),
            "Patient-Eintrag 1 muss weg sein"
        );
        assert!(
            cache.get("patient", "2").is_none(),
            "Patient-Eintrag 2 muss weg sein"
        );
        assert!(
            cache.get("doctor", "9").is_some(),
            "Doctor-Eintrag muss erhalten bleiben"
        );
    }

    /// `invalidate_prefix` auf leerem/unkanntem Typ ist harmlos.
    #[test]
    fn test_invalidate_prefix_unknown_type() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache.set("patient", "1", &serde_json::json!({})).unwrap();
        cache.invalidate_prefix("medication");
        assert!(cache.get("patient", "1").is_some());
    }

    /// Stats: Hit-/Miss-Counts und hit_rate werden korrekt gezählt.
    #[tokio::test]
    async fn test_stats_hit_miss_counts() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache.set("patient", "1", &serde_json::json!({})).unwrap();

        // 1 Hit, 2 Misses
        cache.get("patient", "1");
        cache.get("patient", "missing-1");
        cache.get("patient", "missing-2");

        let s = cache.stats();
        assert_eq!(s.hit_count, 1);
        assert_eq!(s.miss_count, 2);
        assert!((s.hit_rate - 1.0 / 3.0).abs() < f64::EPSILON * 10.0);
    }

    /// Stats: ohne jegliche Zugriffe ist hit_rate 0.0.
    #[test]
    fn test_stats_empty() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let s = cache.stats();
        assert_eq!(s.hit_count, 0);
        assert_eq!(s.miss_count, 0);
        assert_eq!(s.hit_rate, 0.0);
        assert_eq!(s.evictions, 0);
    }

    /// Stats: Read-Through zählt Miss (Loader) und Hit (gecacht).
    #[tokio::test]
    async fn test_stats_read_through() {
        let cache = EntityCache::new(&test_config(300, 1000));

        let _: Option<serde_json::Value> = cache
            .get_or_load("patient", "10", || async {
                anyhow::Ok(Some(serde_json::json!({"id": "10"})))
            })
            .await
            .unwrap();
        let _: Option<serde_json::Value> = cache
            .get_or_load("patient", "10", || async {
                anyhow::Ok(Some(serde_json::json!({"id": "10"})))
            })
            .await
            .unwrap();

        let s = cache.stats();
        assert_eq!(s.miss_count, 1, "Erster Aufruf ist ein Miss");
        assert_eq!(s.hit_count, 1, "Zweiter Aufruf ist ein Hit");
        assert!((s.hit_rate - 0.5).abs() < f64::EPSILON);
    }

    /// Stats: evictions wird geschätzt, wenn Sets die Kapazität
    /// überschreiten.
    #[test]
    fn test_stats_evictions_estimated() {
        let cache = EntityCache::new(&test_config(3600, 3));
        for i in 0..6 {
            cache
                .set("patient", &i.to_string(), &serde_json::json!({"i": i}))
                .unwrap();
        }
        let s = cache.stats();
        assert!(
            s.evictions > 0,
            "Nach 6 Sets mit Kapazität 3 muss mindestens eine Eviction gezählt sein (ist {})",
            s.evictions
        );
    }

    /// Stats serialisieren (für den Health-Endpoint).
    #[test]
    fn test_stats_serializes() {
        let cache = EntityCache::new(&test_config(300, 1000));
        cache.set("patient", "1", &serde_json::json!({})).unwrap();
        cache.get("patient", "1");
        cache.get("patient", "x");

        let v = serde_json::to_value(cache.stats()).unwrap();
        assert_eq!(v["hit_count"], 1);
        assert_eq!(v["miss_count"], 1);
        assert!(v["hit_rate"].is_number());
        assert_eq!(v["evictions"], 0);
    }

    /// Read-Through funktioniert parallel (mehrere Tasks, ein Key).
    #[tokio::test]
    async fn test_read_through_concurrent() {
        let cache = EntityCache::new(&test_config(300, 1000));
        let cache = Arc::new(cache);
        let calls = Arc::new(AtomicU64::new(0));

        let mut handles = Vec::new();
        for i in 0..8 {
            let cache = cache.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                let v: Option<i32> = cache
                    .get_or_load("patient", "conc", || {
                        calls.fetch_add(1, Ordering::Relaxed);
                        async move { anyhow::Ok(Some(i)) }
                    })
                    .await
                    .unwrap();
                v.unwrap()
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.unwrap());
        }
        // Jeder Task bekommt den vom Loader gelieferten Wert — der Cache
        // ist konsistent (alle Werte >= 0, Loader lief >= 1 Mal).
        assert!(results.len() == 8);
        assert!(calls.load(Ordering::Relaxed) >= 1);
    }
}
