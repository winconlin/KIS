//! Anfragebegrenzung je Client-IP (Sliding-Window-Zähler, je Instanz).
//!
//! Sparsam: je Client und Klasse (Anmeldung / übrige Anfragen) nur zwei
//! Zähler und ein Zeitstempel statt einer Liste aller Anfragen. Der Wert
//! des gleitenden Fensters wird aus aktuellem und vorigem Minutenfenster
//! gewichtet berechnet. Abgelaufene Einträge werden einmal je Minute
//! entfernt; der Speicherbedarf folgt damit der Zahl der in den letzten
//! zwei Minuten aktiven Clients.
//!
//! Der Schlüssel ist bewusst **nicht** aus dem (an dieser Stelle
//! unverifizierten) Token abgeleitet: Ein Angreifer könnte sonst mit
//! beliebigen Werten unbegrenzt neue Zähler erzeugen. `X-Forwarded-For`
//! zählt nur, wenn die Verbindung von einem lokalen Reverse-Proxy kommt.
//!
//! Bei mehreren Instanzen gilt das Limit je Instanz.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::config::RateLimitConfig;

const WINDOW: Duration = Duration::from_secs(60);

/// Identität eines Clients für die Begrenzung.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateLimitKey {
    Ip(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allowed { remaining: u32 },
    Blocked { retry_after_secs: u64 },
}

#[derive(Debug, Clone, Copy)]
struct Counter {
    window_start: Instant,
    current: u32,
    previous: u32,
}

#[derive(Debug)]
struct State {
    counters: HashMap<(RateLimitKey, bool), Counter>,
    last_sweep: Instant,
}

#[derive(Debug)]
pub struct SlidingWindowRateLimiter {
    normal_limit: u32,
    auth_limit: u32,
    state: Mutex<State>,
}

impl SlidingWindowRateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            normal_limit: config.normal_per_min.max(1),
            auth_limit: config.auth_per_min.max(1),
            state: Mutex::new(State {
                counters: HashMap::new(),
                last_sweep: Instant::now(),
            }),
        }
    }

    /// Prüft und bucht eine Anfrage. `is_auth` = Anmelde-Endpunkte
    /// (eigener, strengerer Zähler).
    pub async fn check(&self, key: &RateLimitKey, is_auth: bool) -> RateLimitDecision {
        self.check_at(key, is_auth, Instant::now())
    }

    fn check_at(&self, key: &RateLimitKey, is_auth: bool, now: Instant) -> RateLimitDecision {
        let limit = if is_auth {
            self.auth_limit
        } else {
            self.normal_limit
        };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());

        if now.duration_since(state.last_sweep) >= WINDOW {
            state
                .counters
                .retain(|_, c| now.duration_since(c.window_start) < WINDOW * 2);
            state.last_sweep = now;
        }

        let counter = state
            .counters
            .entry((key.clone(), is_auth))
            .or_insert(Counter {
                window_start: now,
                current: 0,
                previous: 0,
            });
        let elapsed = now.duration_since(counter.window_start);
        if elapsed >= WINDOW * 2 {
            *counter = Counter {
                window_start: now,
                current: 0,
                previous: 0,
            };
        } else if elapsed >= WINDOW {
            counter.previous = counter.current;
            counter.current = 0;
            counter.window_start += WINDOW;
        }

        let into_window =
            now.duration_since(counter.window_start).as_secs_f64() / WINDOW.as_secs_f64();
        let weighted =
            f64::from(counter.previous) * (1.0 - into_window) + f64::from(counter.current);
        if weighted >= f64::from(limit) {
            let retry = (WINDOW.as_secs_f64() * (1.0 - into_window)).ceil() as u64;
            tracing::warn!(?key, limit, is_auth, "Rate-Limit überschritten (429)");
            return RateLimitDecision::Blocked {
                retry_after_secs: retry.clamp(1, 60),
            };
        }
        counter.current += 1;
        RateLimitDecision::Allowed {
            remaining: limit.saturating_sub(weighted as u32 + 1),
        }
    }

    /// Anzahl der gehaltenen Zähler (Überwachung, Tests).
    pub fn key_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .counters
            .len()
    }
}

/// Ermittelt die Client-IP. `X-Forwarded-For` wird nur ausgewertet, wenn
/// die Verbindung von einem lokalen Reverse-Proxy (Loopback) kommt; dann
/// zählt der **letzte** Eintrag (vom Proxy angehängt, nicht fälschbar).
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

/// Middleware: begrenzt Anfragen vor der Authentifizierung.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<Arc<SlidingWindowRateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    let is_auth = req.uri().path().starts_with("/api/v1/auth/");
    let peer_addr = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0);
    let key = extract_rate_limit_key(req.headers(), peer_addr);

    match limiter.check(&key, is_auth).await {
        RateLimitDecision::Allowed { .. } => next.run(req).await,
        RateLimitDecision::Blocked { retry_after_secs } => Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::RETRY_AFTER, retry_after_secs.to_string())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "success": false,
                    "error": "Zu viele Anfragen. Bitte später erneut versuchen.",
                    "status": 429,
                })
                .to_string(),
            ))
            .unwrap_or_else(|_| StatusCode::TOO_MANY_REQUESTS.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter() -> SlidingWindowRateLimiter {
        SlidingWindowRateLimiter::new(&RateLimitConfig {
            normal_per_min: 100,
            auth_per_min: 10,
        })
    }

    fn ip(s: &str) -> RateLimitKey {
        RateLimitKey::Ip(s.into())
    }

    #[test]
    fn blocks_after_limit_within_window() {
        let l = limiter();
        let t0 = Instant::now();
        let k = ip("10.0.0.1");
        for i in 0..100 {
            assert!(
                matches!(l.check_at(&k, false, t0), RateLimitDecision::Allowed { .. }),
                "Anfrage {i} erlaubt"
            );
        }
        match l.check_at(&k, false, t0) {
            RateLimitDecision::Blocked { retry_after_secs } => {
                assert!((1..=60).contains(&retry_after_secs))
            }
            other => panic!("erwartet Blocked, war {other:?}"),
        }
    }

    #[test]
    fn auth_and_normal_requests_have_separate_counters() {
        let l = limiter();
        let t0 = Instant::now();
        let k = ip("10.0.0.2");
        for _ in 0..50 {
            l.check_at(&k, false, t0);
        }
        // Normale Last darf die Anmeldung nicht sperren.
        for _ in 0..10 {
            assert!(matches!(
                l.check_at(&k, true, t0),
                RateLimitDecision::Allowed { .. }
            ));
        }
        assert!(matches!(
            l.check_at(&k, true, t0),
            RateLimitDecision::Blocked { .. }
        ));
        assert!(matches!(
            l.check_at(&k, false, t0),
            RateLimitDecision::Allowed { .. }
        ));
    }

    #[test]
    fn keys_are_independent() {
        let l = limiter();
        let t0 = Instant::now();
        for _ in 0..11 {
            l.check_at(&ip("a"), true, t0);
        }
        assert!(matches!(
            l.check_at(&ip("b"), true, t0),
            RateLimitDecision::Allowed { .. }
        ));
    }

    #[test]
    fn window_slides_and_recovers() {
        let l = limiter();
        let t0 = Instant::now();
        let k = ip("10.0.0.3");
        for _ in 0..10 {
            l.check_at(&k, true, t0);
        }
        assert!(matches!(
            l.check_at(&k, true, t0),
            RateLimitDecision::Blocked { .. }
        ));
        // Kurz nach dem Fensterwechsel zählt das alte Fenster noch fast
        // voll: genau eine Anfrage passt, die nächste nicht.
        let t1 = t0 + Duration::from_secs(61);
        assert!(matches!(
            l.check_at(&k, true, t1),
            RateLimitDecision::Allowed { .. }
        ));
        assert!(matches!(
            l.check_at(&k, true, t1),
            RateLimitDecision::Blocked { .. }
        ));
        // Nach zwei weiteren Fenstern ist der Client wieder frei.
        assert!(matches!(
            l.check_at(&k, true, t0 + Duration::from_secs(200)),
            RateLimitDecision::Allowed { .. }
        ));
    }

    #[test]
    fn stale_counters_are_swept() {
        let l = limiter();
        let t0 = Instant::now();
        for i in 0..500 {
            l.check_at(&ip(&format!("10.1.{}.{}", i / 250, i % 250)), false, t0);
        }
        assert_eq!(l.key_count(), 500);
        l.check_at(&ip("neu"), false, t0 + Duration::from_secs(180));
        assert_eq!(l.key_count(), 1, "abgelaufene Zähler werden entfernt");
    }

    #[test]
    fn key_ignores_bearer_token() {
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            "Bearer x.eyJzdWIiOiJhIn0.y".parse().unwrap(),
        );
        let peer: std::net::SocketAddr = "203.0.113.7:5000".parse().unwrap();
        assert_eq!(extract_rate_limit_key(&h, Some(peer)), ip("203.0.113.7"));
    }

    #[test]
    fn forwarded_for_only_from_loopback() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-forwarded-for", "1.1.1.1, 198.51.100.4".parse().unwrap());
        let external: std::net::SocketAddr = "203.0.113.7:5000".parse().unwrap();
        let proxy: std::net::SocketAddr = "127.0.0.1:5000".parse().unwrap();
        assert_eq!(
            extract_rate_limit_key(&h, Some(external)),
            ip("203.0.113.7")
        );
        assert_eq!(extract_rate_limit_key(&h, Some(proxy)), ip("198.51.100.4"));
    }
}
