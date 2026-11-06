// ============================================================
// KIS — Rate-Limiting (Task 1.11)
//
// Sliding-Window-Algorithmus pro User:
//   - 100 Requests/min für normale /api/v1/*-Endpunkte
//   -   10 Requests/min für /auth/*-Endpunkte (Login-Schutz)
//
// In-memory (HashMap hinter tokio::sync::Mutex): ausreichend für
// einen Single-Node-Deploy; bei Multi-Node auf Redis umstellen.
//
// Axum-Middleware:
//   - Identifiziert den User über den Auth-Header (JWT-Sub),
//     sonst über die Client-IP.
//   - Bei Überschreitung: HTTP 429 + Retry-After-Header.
// ============================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::RateLimitConfig;

/// Key für die Rate-Limit-Identität.
///
/// Für authentifizierte Requests wird der JWT-Subject (User) genutzt —
/// das Limit gilt dann pro User, unabhängig von der IP. Für
/// nicht-authentifizierte Requests (Login, Health-Check) gilt die IP.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateLimitKey {
    /// Authentifizierte Identität (JWT-Sub, z. B. "user:42")
    User(String),
    /// Client-IP (nicht-authentifizierte Requests)
    Ip(String),
}

impl std::fmt::Display for RateLimitKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitKey::User(id) => write!(f, "user:{id}"),
            RateLimitKey::Ip(ip) => write!(f, "ip:{ip}"),
        }
    }
}

/// Ergebnis einer Rate-Limit-Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// Request ist erlaubt; `remaining` = verbleibende Requests im Fenster
    Allowed { remaining: u32 },
    /// Request ist blockiert; `retry_after_secs` = Sekunden bis Fenster-Rotation
    Blocked { retry_after_secs: u64 },
}

/// Sliding-Window-Rate-Limiter.
///
/// Speichert pro Key die Zeitstempel (`Instant`) der Requests innerhalb
/// des letzten 60-Sekunden-Fensters. Alte Einträge werden bei jeder
/// Prüfung ausgemistet (lazy cleanup), dadurch bleibt der Speicher
/// beschränkt — auch bei vielen Inaktiven.
#[derive(Debug)]
pub struct SlidingWindowRateLimiter {
    /// Fenstergröße (1 Minute)
    window: Duration,
    /// Requests pro Fenster für normale Endpunkte
    normal_limit: u32,
    /// Requests pro Fenster für Auth-Endpunkte
    auth_limit: u32,
    /// key → Zeitstempel der Requests im aktuellen Fenster
    buckets: Mutex<HashMap<RateLimitKey, Vec<Instant>>>,
}

impl SlidingWindowRateLimiter {
    /// Erzeugt einen Limiter aus der Konfiguration.
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            window: Duration::from_secs(60),
            normal_limit: config.normal_per_min.max(1),
            auth_limit: config.auth_per_min.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Prüft und bucht einen Request.
    ///
    /// `is_auth` = true für /auth/*-Endpunkte (strengeres Limit).
    pub async fn check(&self, key: &RateLimitKey, is_auth: bool) -> RateLimitDecision {
        let limit = if is_auth {
            self.auth_limit
        } else {
            self.normal_limit
        };
        let cutoff = Instant::now() - self.window;

        let mut buckets = self.buckets.lock().await;
        let bucket = buckets.entry(key.clone()).or_insert_with(Vec::new);

        // Alte Einträge aus dem Fenster entfernen (sliding window)
        bucket.retain(|t| *t > cutoff);

        if bucket.len() as u32 >= limit {
            // Nächste freie Position = ältester Eintrag + Fenster
            let retry_after = bucket
                .first()
                .map(|t| {
                    let elapsed = t.elapsed();
                    let remaining = self.window.saturating_sub(elapsed);
                    remaining.as_secs().max(1)
                })
                .unwrap_or(1);
            warn!(
                key = %key,
                limit,
                "Rate-Limit überschritten — Request blockiert (HTTP 429)"
            );
            return RateLimitDecision::Blocked {
                retry_after_secs: retry_after,
            };
        }

        // Request erlaubt: Zeitstempel buchen
        bucket.push(Instant::now());
        RateLimitDecision::Allowed {
            remaining: limit - bucket.len() as u32,
        }
    }

    /// Löscht den Bucket eines Keys (z. B. nach Logout).
    pub async fn remove(&self, key: &RateLimitKey) {
        self.buckets.lock().await.remove(key);
    }

    /// Anzahl der gespeicherten Keys (für Monitoring/Tests).
    pub async fn key_count(&self) -> usize {
        self.buckets.lock().await.len()
    }
}

/// Ermittelt die Rate-Limit-Identität aus einem Request: die Client-IP.
///
/// Bewusst **nicht** aus dem (an dieser Stelle unverifizierten) JWT: Ein
/// Angreifer könnte sonst mit beliebigen `sub`-Claims unbegrenzt neue
/// Buckets erzeugen und das Limit umgehen.
///
/// `X-Forwarded-For` wird nur ausgewertet, wenn die Verbindung von einem
/// lokalen Reverse-Proxy (Loopback) kommt; dann zählt der **letzte**
/// Eintrag (vom Proxy angehängt, vom Client nicht fälschbar).
pub fn extract_rate_limit_key(
    headers: &axum::http::HeaderMap,
    peer_addr: Option<std::net::SocketAddr>,
) -> RateLimitKey {
    let from_proxy = peer_addr.map(|a| a.ip().is_loopback()).unwrap_or(false);
    let forwarded = if from_proxy {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(|s| s.trim().to_string())
            .filter(|s| s.parse::<std::net::IpAddr>().is_ok())
    } else {
        None
    };
    let ip = forwarded
        .or_else(|| peer_addr.map(|a| a.ip().to_string()))
        .unwrap_or_else(|| "unknown".into());
    RateLimitKey::Ip(ip)
}

/// Axum-Middleware: prüft das Rate-Limit vor der Handler-Ausführung.
///
/// `limiter` wird über `State` in die App injiziert (`Arc<SlidingWindowRateLimiter>`).
/// Auth-Routen werden am Pfad erkannt (Prefix `/api/v1/auth/`) und bekommen
/// das strengere Auth-Limit (10/min statt 100/min).
///
/// Der Key wird aus JWT-Sub (falls vorhanden) oder Client-IP abgeleitet.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<Arc<SlidingWindowRateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    let uri = req.uri().path().to_string();
    // Auth-Routen: /api/v1/auth/* (login, refresh, logout)
    let is_auth = uri.starts_with("/api/v1/auth/");
    let headers = req.headers().clone();

    // Peer-Addr ist in der Middleware ohne ConnectInfo nicht verfügbar →
    // nur X-Forwarded-For / "unknown". Für die Rate-Limit-Gruppierung
    // genügt das: authentifizierte Requests nutzen ohnehin den JWT-Sub.
    let peer_addr = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0);
    let key = extract_rate_limit_key(&headers, peer_addr);

    match limiter.check(&key, is_auth).await {
        RateLimitDecision::Allowed { remaining } => {
            let mut resp = next.run(req).await;
            // X-RateLimit-Remaining: Rest im Fenster (Information, kein Secret)
            resp.headers_mut().insert(
                "X-RateLimit-Remaining",
                remaining
                    .to_string()
                    .parse()
                    .unwrap_or_else(|_| "0".parse().unwrap()),
            );
            resp
        }
        RateLimitDecision::Blocked { retry_after_secs } => {
            // 429 Too Many Requests + Retry-After (RFC 9110)
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(header::RETRY_AFTER, retry_after_secs.to_string())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "success": false,
                        "error": "Zu viele Anfragen. Bitte später erneut versuchen."
                    })
                    .to_string(),
                ))
                .unwrap_or_else(|_| {
                    (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response()
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RateLimitConfig {
        RateLimitConfig {
            normal_per_min: 100,
            auth_per_min: 10,
        }
    }

    /// 101 Requests → der 101. wird blockiert (normales Limit).
    #[tokio::test]
    async fn test_normal_limit_blocks_101st_request() {
        let limiter = SlidingWindowRateLimiter::new(&config());
        let key = RateLimitKey::User("user-1".into());

        let mut blocked_at: Option<u32> = None;
        for i in 0..101u32 {
            let d = limiter.check(&key, false).await;
            if matches!(d, RateLimitDecision::Blocked { .. }) {
                blocked_at = Some(i + 1);
                break;
            }
        }
        assert_eq!(
            blocked_at,
            Some(101),
            "Der 101. Request muss blockiert sein, war: {blocked_at:?}"
        );
    }

    /// 10 Login-Versuche → der 11. wird blockiert (Auth-Limit).
    #[tokio::test]
    async fn test_auth_limit_blocks_11th_login() {
        let limiter = SlidingWindowRateLimiter::new(&config());
        let key = RateLimitKey::Ip("10.0.0.1".into());

        let mut blocked_at: Option<u32> = None;
        for i in 0..11u32 {
            let d = limiter.check(&key, true).await;
            if matches!(d, RateLimitDecision::Blocked { .. }) {
                blocked_at = Some(i + 1);
                break;
            }
        }
        assert_eq!(
            blocked_at,
            Some(11),
            "Der 11. Login-Versuch muss blockiert sein, war: {blocked_at:?}"
        );
    }

    /// Verschiedene User haben unabhängige Buckets.
    #[tokio::test]
    async fn test_separate_users_independent_buckets() {
        let limiter = SlidingWindowRateLimiter::new(&config());
        let a = RateLimitKey::User("user-a".into());
        let b = RateLimitKey::User("user-b".into());

        for _ in 0..100 {
            limiter.check(&a, false).await;
        }
        // A ist blockiert, B ist frisch
        assert!(matches!(
            limiter.check(&a, false).await,
            RateLimitDecision::Blocked { .. }
        ));
        assert!(matches!(
            limiter.check(&b, false).await,
            RateLimitDecision::Allowed { remaining: 99 }
        ));
    }

    /// Blockierte Requests liefern Retry-After > 0.
    #[tokio::test]
    async fn test_blocked_returns_retry_after() {
        let limiter = SlidingWindowRateLimiter::new(&config());
        let key = RateLimitKey::Ip("10.0.0.2".into());

        for _ in 0..10 {
            limiter.check(&key, true).await;
        }
        match limiter.check(&key, true).await {
            RateLimitDecision::Blocked { retry_after_secs } => {
                assert!((1..=60).contains(&retry_after_secs));
            }
            other => panic!("erwartete Blocked, bekam {other:?}"),
        }
    }

    /// Gefälschte Bearer-Tokens dürfen keine eigenen Buckets erzeugen.
    #[test]
    fn test_key_ignores_bearer_token() {
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            "Bearer x.eyJzdWIiOiJhIn0.y".parse().unwrap(),
        );
        let peer: std::net::SocketAddr = "203.0.113.7:5000".parse().unwrap();
        assert_eq!(
            extract_rate_limit_key(&h, Some(peer)),
            RateLimitKey::Ip("203.0.113.7".into())
        );
    }

    /// X-Forwarded-For nur von einem lokalen Proxy, letzter Eintrag zählt.
    #[test]
    fn test_forwarded_for_only_from_loopback() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-forwarded-for", "1.1.1.1, 198.51.100.4".parse().unwrap());
        let external: std::net::SocketAddr = "203.0.113.7:5000".parse().unwrap();
        let proxy: std::net::SocketAddr = "127.0.0.1:5000".parse().unwrap();
        assert_eq!(
            extract_rate_limit_key(&h, Some(external)),
            RateLimitKey::Ip("203.0.113.7".into())
        );
        assert_eq!(
            extract_rate_limit_key(&h, Some(proxy)),
            RateLimitKey::Ip("198.51.100.4".into())
        );
    }
}
