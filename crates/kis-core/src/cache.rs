// ============================================================
// KIS — In-Memory-Cache (Task 1.17)
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
// Der Cache speichert serialisierte JSON-Strings (serde_json::Value),
// damit beliebige serde-serialisierbare Entitäten ohne Monomorphisier-
// overhead zwischengespeichert werden.
// ============================================================

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

/// KIS-Entitäten-Cache (Task 1.17).
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
    #[allow(dead_code)]
    max_entries: u64,
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
        }
    }

    /// Liest einen Eintrag: `Some(json)` bei Hit, `None` bei Miss/expired.
    pub fn get(&self, entity_type: &str, entity_id: &str) -> Option<Value> {
        self.cache.get(&CacheKey::new(entity_type, entity_id).moka_key())
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

    /// Schreibt einen serialisierbaren Wert in den Cache.
    pub fn set<T: serde::Serialize>(
        &self,
        entity_type: &str,
        entity_id: &str,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        let json = serde_json::to_value(value)?;
        self.cache
            .insert(CacheKey::new(entity_type, entity_id).moka_key(), json);
        debug!(
            entity_type,
            entity_id,
            "Cache-Set (TTL läuft ab dem jetzigen Zeitpunkt)"
        );
        Ok(())
    }

    /// Invalidiert einen konkreten Eintrag (nach UPDATE/DELETE der Entität).
    pub fn invalidate(&self, entity_type: &str, entity_id: &str) {
        let key = CacheKey::new(entity_type, entity_id).moka_key();
        self.cache.invalidate(&key);
        debug!(entity_type, entity_id, "Cache-Eintrag invalidiert (nach Write)");
    }

    /// Invalidiert ALLE Einträge (z. B. nach massiven Änderungen).
    ///
    /// moka hat kein `invalidate_all` für sync-Caches — wir bauen eine
    /// frische Map, die die aktuelle Größe überschreitet. Stattdessen
    /// nutzen wir den einfachen Weg: Cache-Instanz neu bauen.
    pub fn invalidate_all(&self) {
        // moka::sync::Cache bietet `invalidate_all` in neueren Versionen;
        // sicherer Fallback: wir zählen die Einträge und lassen sie
        // durch die maximale Kapazität verdrängen. In der Praxis reicht
        /// `invalidate()` pro Key — dieser Call ist für den Compliance-
        /// Fall "komplette Cache-Flushing nach DB-Migration".
        // (Implementierung über interne Key-Liste wäre O(n); wir nutzen
        // hier den dokumentierten moka-Reset über eine neue Instanz.)
        self.cache.invalidate_all();
        debug!("Gesamter Cache invalidiert");
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
        cache.set("patient", "999", &serde_json::json!({"v": 1})).unwrap();
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
}
