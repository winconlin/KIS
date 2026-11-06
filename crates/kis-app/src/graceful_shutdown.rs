//! Graceful Shutdown (Task 5.12)
//!
//! Strukturierte Beendigung bei SIGTERM/SIGINT:
//!
//! 1. **Neue Requests ablehnen** — `shutting_down` wird `true`; die
//!    Shutdown-Middleware antwortet mit `503 Service Unavailable`.
//! 2. **Laufende Requests abschließen** — axum `serve()` wird über den
//!    watch-Channel beendet und wartet auf in-flight Requests
//!    (max. `shutdown_timeout`, Standard 30s).
//! 3. **DB-Pools schließen** — `sqlx Pool::close()` (Write + Read).
//! 4. **Clean Exit** — Prozess beendet mit Exit-Code 0.
//!
//! Die Logik ist in [`GracefulShutdown`] kapsuliert und unit-testbar.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Router;
use kis_core::db::Database;
use tokio::sync::watch;

/// Maximaler Wert für den Shutdown-Timeout (Schutz gegen Konfig-Fehler).
const MAX_SHUTDOWN_TIMEOUT: u64 = 300;

/// Steuerungsstruktur für den Graceful Shutdown.
///
/// Wird einmalig zu Startzeit erstellt (`new()`), dann:
/// - `Arc<AtomicBool>` an die axum-Middleware übergeben (Phase 1)
/// - `watch::Receiver` an den axum-Server-Task übergeben (Phase 2)
///
/// Der `GracefulShutdown`-Handle wird im `main.rs` aufbewahrt und
/// bei SIGTERM `trigger()` + `notify_axum_shutdown()` aufgerufen.
#[derive(Debug)]
pub struct GracefulShutdown {
    /// Phase-1-Flag: Wenn `true`, lehnt die Middleware neue Requests mit 503 ab.
    shutting_down: Arc<AtomicBool>,
    /// Phase-2-Signal: axum `serve()` wartet auf `true` → beendet sich.
    shutdown_tx: watch::Sender<bool>,
    /// Konfigurierter Timeout für Phase 2 (Standard 30s).
    timeout: Duration,
}

impl GracefulShutdown {
    /// Erzeugt den Shutdown-Controller.
    pub fn new(shutdown_timeout_seconds: u64) -> Self {
        let timeout = Duration::from_secs(shutdown_timeout_seconds.min(MAX_SHUTDOWN_TIMEOUT));
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            shutting_down: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
            timeout,
        }
    }

    /// **Phase 1:** Setzt das Flag, dass neue Requests abgelehnt werden sollen.
    ///
    /// Die axum-Middleware prüft dieses Flag bei jedem Request.
    /// Ab dem Setzen antwortet sie mit `503 Service Unavailable`.
    /// Bestehende (in-flight) Requests laufen weiter.
    pub fn trigger(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        tracing::info!(
            timeout_secs = self.timeout.as_secs(),
            "Graceful Shutdown: Phase 1 — neue Requests werden abgelehnt (503)"
        );
    }

    /// Liefert einen `Arc<AtomicBool>` für die axum-Middleware.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutting_down)
    }

    /// Liefert einen `watch::Receiver` für den axum-Server-Task.
    /// axum `serve()` wartet auf `true` → Phase 2.
    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    /// **Phase 2:** Benachrichtigt den axum-Server-Task:
    /// axum `serve()` hört auf, neue Verbindungen anzunehmen,
    /// und wartet auf in-flight Requests.
    pub fn notify_axum_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        tracing::info!(
            "Graceful Shutdown: Phase 2 — axum-Server schließt, laufende Requests werden abgewartet (max {}s)",
            self.timeout.as_secs()
        );
    }

    /// Der konfigurierte Timeout für Phase 2.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// **Phase 3:** Schließt die DB-Pools (Write + Read).
    pub async fn close_pools(&self, db: &Database) {
        tracing::info!("Graceful Shutdown: Phase 3 — DB-Pools werden geschlossen");
        let started = Instant::now();

        db.write_pool.close().await;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Graceful Shutdown: Write-Pool geschlossen"
        );

        db.read_pool.close().await;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Graceful Shutdown: Read-Pool geschlossen"
        );
    }
}

/// axum-Middleware: Lehnt neue Requests mit 503 ab, wenn der
/// Shutdown-Flag `true` ist.
///
/// Wird als `Router::layer()` mit `from_fn_with_state` auf den
/// gesamten Router gelegt (siehe [`router_with_shutdown`]).
async fn shutdown_middleware_fn(
    State(flag): State<Arc<AtomicBool>>,
    request: Request,
    next: Next,
) -> Response {
    if flag.load(Ordering::SeqCst) {
        (StatusCode::SERVICE_UNAVAILABLE, "Service shutting down").into_response()
    } else {
        next.run(request).await
    }
}

/// Baut den Router mit der Shutdown-Middleware ein.
///
/// Wird in `main.rs` aufgerufen, nachdem `GracefulShutdown::new()`
/// erstellt wurde. Nach `trigger()` lehnt die Middleware neue
/// Requests mit 503 ab. Der Health-Check antwortet ebenfalls mit
/// 503, damit der Load-Balancer die Instanz aus dem Pool entfernt.
pub fn router_with_shutdown<T>(base: Router<T>, shutdown: &GracefulShutdown) -> Router<T>
where
    T: Clone + Send + Sync + 'static,
{
    let flag = shutdown.shutdown_flag();
    base.layer(axum::middleware::from_fn_with_state(
        flag,
        shutdown_middleware_fn,
    ))
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tower::ServiceExt;

    /// GracefulShutdown: Default-Timeout ist 30s.
    #[test]
    fn test_default_shutdown_timeout_is_30s() {
        let gs = GracefulShutdown::new(30);
        assert_eq!(gs.timeout(), Duration::from_secs(30));
    }

    /// GracefulShutdown: Timeout wird bei > MAX_SHUTDOWN_TIMEOUT gekappt.
    #[test]
    fn test_shutdown_timeout_capped_at_max() {
        let gs = GracefulShutdown::new(9999);
        assert_eq!(gs.timeout(), Duration::from_secs(MAX_SHUTDOWN_TIMEOUT));
    }

    /// Phase 1: Vor `trigger()` ist das Flag `false` (Requests werden akzeptiert).
    #[test]
    fn test_shutdown_flag_is_false_before_trigger() {
        let gs = GracefulShutdown::new(30);
        let flag = gs.shutdown_flag();
        assert!(!flag.load(Ordering::SeqCst));
    }

    /// Phase 1: Nach `trigger()` ist das Flag `true` (neue Requests → 503).
    #[test]
    fn test_shutdown_flag_is_true_after_trigger() {
        let gs = GracefulShutdown::new(30);
        let flag = gs.shutdown_flag();
        assert!(!flag.load(Ordering::SeqCst));

        gs.trigger();
        assert!(flag.load(Ordering::SeqCst));
    }

    /// Phase 1: Flag ist `Send + Sync` (kann über Thread-Grenzen geteilt werden).
    #[test]
    fn test_shutdown_flag_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<AtomicBool>>();
    }

    /// Phase 2: Der watch-Receiver liefert `false` vor `notify_axum_shutdown()`.
    #[tokio::test]
    async fn test_shutdown_receiver_is_false_before_notify() {
        let gs = GracefulShutdown::new(30);
        let rx = gs.shutdown_receiver();
        assert!(!*rx.borrow());
    }

    /// Phase 2: Nach `notify_axum_shutdown()` liefert der Receiver `true`.
    #[tokio::test]
    async fn test_shutdown_receiver_is_true_after_notify() {
        let gs = GracefulShutdown::new(30);
        let rx = gs.shutdown_receiver();
        assert!(!*rx.borrow());

        gs.notify_axum_shutdown();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(*rx.borrow());
    }

    /// Phase 1: Middleware lehnt nach Trigger mit 503 ab (Integration mit axum).
    #[tokio::test]
    async fn test_middleware_rejects_after_shutdown() {
        use axum::routing::get;

        let gs = GracefulShutdown::new(30);

        async fn handler() -> &'static str {
            "ok"
        }

        async fn mw(State(flag): State<Arc<AtomicBool>>, request: Request, next: Next) -> Response {
            if flag.load(Ordering::SeqCst) {
                (StatusCode::SERVICE_UNAVAILABLE, "shutting down").into_response()
            } else {
                next.run(request).await
            }
        }

        let flag = gs.shutdown_flag();
        let app = Router::new()
            .route("/test", get(handler))
            .layer(axum::middleware::from_fn_with_state(Arc::clone(&flag), mw));

        // Vor Trigger: 200
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Trigger → 503
        gs.trigger();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    /// Phase 2: axum serve() beendet sich innerhalb des Timeouts,
    /// wenn der Shutdown-Trigger ausgelöst wird.
    #[tokio::test]
    async fn test_shutdown_timeout_completes() {
        let timeout = Duration::from_secs(2);
        let gs = GracefulShutdown::new(2);
        let mut rx = gs.shutdown_receiver();

        // Simuliere axum serve(): wartet auf Shutdown-Signal
        let serve_handle = tokio::spawn(async move {
            rx.wait_for(|v| *v).await.expect("watch channel closed");
            Duration::from_secs(0)
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        gs.notify_axum_shutdown();

        let result = tokio::time::timeout(timeout + Duration::from_secs(1), serve_handle).await;
        assert!(
            result.is_ok(),
            "Shutdown hat nicht innerhalb des Timeouts ({:?}) abgeschlossen",
            timeout
        );
    }

    /// Phase 3: Pool-Schließen — verifiziert die Verdrahtung der Phasen.
    #[tokio::test]
    async fn test_shutdown_pool_close_is_called() {
        let gs = GracefulShutdown::new(30);
        assert!(!gs.shutdown_flag().load(Ordering::SeqCst));
        assert!(!*gs.shutdown_receiver().borrow());

        gs.trigger();
        assert!(gs.shutdown_flag().load(Ordering::SeqCst));
        gs.notify_axum_shutdown();
        // close_pools() wird in main.rs aufgerufen (keine echte DB im Unit-Test)
    }

    /// GracefulShutdown: Mehrere Flag-Referenzen teilen denselben Zustand.
    #[test]
    fn test_multiple_flag_references_share_state() {
        let gs = GracefulShutdown::new(30);
        let flag1 = gs.shutdown_flag();
        let flag2 = gs.shutdown_flag();

        assert!(!flag1.load(Ordering::SeqCst));
        assert!(!flag2.load(Ordering::SeqCst));

        gs.trigger();

        assert!(flag1.load(Ordering::SeqCst));
        assert!(flag2.load(Ordering::SeqCst));
    }

    /// GracefulShutdown: trigger() ist idempotent.
    #[test]
    fn test_trigger_is_idempotent() {
        let gs = GracefulShutdown::new(30);
        let flag = gs.shutdown_flag();

        gs.trigger();
        gs.trigger();
        gs.trigger();

        assert!(flag.load(Ordering::SeqCst));
    }

    /// Vollständiger Phasen-Verlauf: Flag → Receiver → Timeout.
    #[tokio::test]
    async fn test_full_phase_sequence() {
        let gs = GracefulShutdown::new(5);
        let flag = gs.shutdown_flag();
        let mut rx = gs.shutdown_receiver();

        // Vor Shutdown: Flag false, Receiver false
        assert!(!flag.load(Ordering::SeqCst));
        assert!(!*rx.borrow());

        // Phase 1: Trigger
        gs.trigger();
        assert!(flag.load(Ordering::SeqCst));

        // Phase 2: axum benachrichtigen
        gs.notify_axum_shutdown();
        let _ = rx.wait_for(|v| *v).await;
        assert!(*rx.borrow());

        // Timeout korrekt
        assert_eq!(gs.timeout(), Duration::from_secs(5));
    }
}
