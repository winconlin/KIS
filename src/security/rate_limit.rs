//! In-memory Rate-Limiting: gleitendes Fenster (sliding window) pro IP
//!
//! Standard: 100 Requests/Minute pro IP; Auth-Endpunkte: 10/Minute.
//! Überschrittene Limits beantworten 429 mit Retry-After-Header.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Ergebnis einer Rate-Limit-Prüfung
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Anfrage erlaubt
    Allowed,
    /// Limit erreicht — Retry-After in Sekunden
    RetryAfter(u64),
}

/// Gleitendes-Fenster-Limiter: zählt Requests in den letzten 60 Sekunden
pub struct SlidingWindowLimiter {
    /// requests_per_minute je Scope
    limits: HashMap<String, u32>,
    /// ip -> Liste der Zeitstempel der Requests im aktuellen Fenster
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl SlidingWindowLimiter {
    /// Erzeugt einen Limiter mit den Standardlimits (api + auth Scope).
    pub fn new(requests_per_minute: u32, auth_requests_per_minute: u32) -> Self {
        let mut limits = HashMap::new();
        limits.insert("api".to_string(), requests_per_minute);
        limits.insert("auth".to_string(), auth_requests_per_minute);
        Self {
            limits,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Prüft, ob der Request erlaubt ist, und zählt ihn — falls erlaubt.
    ///
    /// `scope`: "api" (Standard) oder "auth" (strenger).
    pub fn check(&self, ip: &str, scope: &str) -> RateLimitResult {
        let limit = *self
            .limits
            .get(scope)
            .unwrap_or(&self.limits.get("api").copied().unwrap_or(100));
        let window = Duration::from_secs(60);

        let mut guard = self.hits.lock().unwrap();
        let key = format!("{scope}:{ip}");
        let now = Instant::now();

        let entries = guard.entry(key).or_default();
        // Altertreibende Einträge aufräumen (Fenster = 60 s)
        entries.retain(|t| now.duration_since(*t) < window);

        if entries.len() as u64 >= limit as u64 {
            // Wie lange bis zum ältesten Eintrag im Fenster verstreicht
            let oldest = entries.first().copied().unwrap_or(now);
            let wait = window.saturating_sub(now.duration_since(oldest));
            drop(guard);
            return RateLimitResult::RetryAfter(wait.as_secs().max(1));
        }

        entries.push(now);
        RateLimitResult::Allowed
    }

    /// Löscht alle Zähler (z. B. zum Testen)
    pub fn reset(&self) {
        self.hits.lock().unwrap().clear();
    }
}

impl Default for SlidingWindowLimiter {
    fn default() -> Self {
        Self::new(100, 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_below_limit() {
        let l = SlidingWindowLimiter::new(3, 2);
        for _ in 0..3 {
            assert_eq!(l.check("1.2.3.4", "api"), RateLimitResult::Allowed);
        }
        assert!(matches!(l.check("1.2.3.4", "api"), RateLimitResult::RetryAfter(_)));
    }

    #[test]
    fn separate_ips_independent() {
        let l = SlidingWindowLimiter::new(1, 1);
        assert_eq!(l.check("a", "api"), RateLimitResult::Allowed);
        assert_eq!(l.check("b", "api"), RateLimitResult::Allowed);
        assert!(matches!(l.check("a", "api"), RateLimitResult::RetryAfter(_)));
    }

    #[test]
    fn auth_scope_stricter() {
        let l = SlidingWindowLimiter::new(100, 2);
        assert_eq!(l.check("a", "auth"), RateLimitResult::Allowed);
        assert_eq!(l.check("a", "auth"), RateLimitResult::Allowed);
        assert!(matches!(l.check("a", "auth"), RateLimitResult::RetryAfter(_)));
        // api-Scope derselben IP bleibt frei
        assert_eq!(l.check("a", "api"), RateLimitResult::Allowed);
    }
}
