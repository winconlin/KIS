//! Task 4.13 — MEDICO-Status-Monitoring
//!
//! `MedicoMonitor`: prüft alle 60s:
//! - **Connectivity**: PING (FHIR-Basicauftritt)
//! - **Latenz**: letzte Antwortzeit in ms
//! - **Fehler**: letzte 100 Requests (FIFO-Ring)
//!
//! `GET /api/v1/interfaces/medico/monitoring` — Dashboard-Daten.
//!
//! Alerting: Wenn 3+ Fehler in Folge → `tracing::error!` + Status `degraded`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kis_core::db::Database;
use serde::Serialize;
use tracing::{error, info, warn};

use crate::client::FhirClient;

// ============================================================
// Monitoring-Datenstrukturen
// ============================================================

/// Gesamtstatus des MEDICO-Adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MedicoHealth {
    /// Alles normal
    Healthy,
    /// 3+ Fehler in Folge — degradierter Betrieb
    Degraded,
    /// MEDICO komplett unerreichbar
    Unreachable,
}

/// Einzelne Request-Statistik.
#[derive(Debug, Clone, Serialize)]
pub struct RequestStat {
    /// Zeitstempel des Requests
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Antwortzeit in ms
    pub latency_ms: u64,
    /// HTTP-Status-Code
    pub status_code: u16,
    /// War es ein Fehler (4xx/5xx oder Timeout)?
    pub is_error: bool,
}

/// Dashboard-Daten für den Monitoring-Endpunkt.
#[derive(Debug, Clone, Serialize)]
pub struct MonitoringData {
    /// Gesamtstatus
    pub health: MedicoHealth,
    /// Anzahl aufeinanderfolgender Fehler
    pub consecutive_errors: u32,
    /// Letzter PING-Erfolg (Zeitstempel)
    pub last_ping_ok: Option<String>,
    /// Letzter PING-Fehler (Zeitstempel)
    pub last_ping_fail: Option<String>,
    /// Letzte Antwortzeit in ms
    pub last_latency_ms: Option<u64>,
    /// Durchschnittliche Latenz (letzte 100 Requests)
    pub avg_latency_ms: Option<u64>,
    /// Anzahl Requests in den letzten 100
    pub request_count: usize,
    /// Anzahl Fehler in den letzten 100 Requests
    pub error_count: u32,
    /// Fehlerquote (0.0–1.0)
    pub error_rate: f64,
    /// Letzte 10 Requests (neueste zuerst)
    pub recent_requests: Vec<RequestStat>,
    /// Zeitstempel der letzten Prüfung
    pub last_check_at: Option<String>,
}

// ============================================================
// MedicoMonitor
// ============================================================

/// MEDICO-Status-Monitor.
///
/// Hält eine FIFO-Queue der letzten 100 Requests und führt
/// alle 60s einen PING-Check durch.
pub struct MedicoMonitor {
    /// FHIR-Client für PING-Checks
    pub client: Arc<FhirClient>,
    /// Gesamtstatus
    pub health: std::sync::atomic::AtomicU8, // 0=healthy, 1=degraded, 2=unreachable
    /// Aufeinanderfolgende Fehler
    pub consecutive_errors: std::sync::atomic::AtomicU32,
    /// Ring-Buffer der letzten 100 Requests
    pub requests: Mutex<VecDeque<RequestStat>>,
    /// Letzter erfolgreicher PING
    pub last_ping_ok: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Letzter fehlgeschlagener PING
    pub last_ping_fail: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Letzte Antwortzeit
    pub last_latency_ms: std::sync::atomic::AtomicU64,
    /// Letzter Check-Zeitstempel
    pub last_check_at: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// db für Audit-Logs (optional)
    pub db: Option<Arc<Database>>,
}

impl MedicoMonitor {
    pub fn new(
        client: Arc<FhirClient>,
        db: Option<Arc<Database>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            client,
            health: std::sync::atomic::AtomicU8::new(0),
            consecutive_errors: std::sync::atomic::AtomicU32::new(0),
            requests: Mutex::new(VecDeque::with_capacity(100)),
            last_ping_ok: Mutex::new(None),
            last_ping_fail: Mutex::new(None),
            last_latency_ms: std::sync::atomic::AtomicU64::new(0),
            last_check_at: Mutex::new(None),
            db,
        })
    }

    /// Registriert einen Request (aus dem FHIR-Client).
    pub fn record_request(&self, latency_ms: u64, status_code: u16) {
        let is_error = status_code >= 400;
        let stat = RequestStat {
            timestamp: chrono::Utc::now(),
            latency_ms,
            status_code,
            is_error,
        };

        let mut queue = self.requests.lock().unwrap();
        queue.push_back(stat);
        if queue.len() > 100 {
            queue.pop_front();
        }

        if is_error {
            self.consecutive_errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.consecutive_errors.store(0, std::sync::atomic::Ordering::Relaxed);
        }

        self.last_latency_ms
            .store(latency_ms, std::sync::atomic::Ordering::Relaxed);

        // Alerting: 3+ Fehler in Folge
        let errors = self
            .consecutive_errors
            .load(std::sync::atomic::Ordering::Relaxed);
        if errors >= 3 {
            let old = self
                .health
                .swap(1, std::sync::atomic::Ordering::Relaxed); // degraded
            if old == 0 {
                error!(
                    consecutive_errors = errors,
                    "MEDICO: 3+ Fehler in Folge — Status auf DEGRADED"
                );
            }
        }
    }

    /// Führt einen PING-Check durch (GET auf FHIR-Basis-URL).
    pub async fn ping(&self) -> Result<bool, String> {
        let start = std::time::Instant::now();
        let result = self.client.ping().await;
        let latency = start.elapsed().as_millis() as u64;

        let now = chrono::Utc::now();
        match &result {
            Ok(_) => {
                *self.last_ping_ok.lock().unwrap() = Some(now);
                *self.last_ping_fail.lock().unwrap() = None;
                self.consecutive_errors
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                if self.health.load(std::sync::atomic::Ordering::Relaxed) != 0 {
                    info!(
                        "MEDICO: PING erfolgreich — Status zurück auf HEALTHY"
                    );
                }
                self.health.store(0, std::sync::atomic::Ordering::Relaxed);
                self.record_request(latency, 200);
                Ok(true)
            }
            Err(e) => {
                *self.last_ping_fail.lock().unwrap() = Some(now);
                *self.last_ping_ok.lock().unwrap() = None;
                warn!(error = %e, "MEDICO: PING fehlgeschlagen");
                self.record_request(latency, 503);
                Err(e.clone())
            }
        }
    }

    /// Startet den Monitoring-Loop (alle 60s).
    ///
    /// Läuft als eigenständiger Tokio-Task.
    pub fn spawn_monitoring_loop(self: Arc<Self>) {
        let monitor = Arc::clone(&self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let result = monitor.ping().await;
                let now = chrono::Utc::now();
                *monitor.last_check_at.lock().unwrap() = Some(now);
                let health = monitor.current_health();
                info!(
                    ?health,
                    consecutive_errors = monitor
                        .consecutive_errors
                        .load(std::sync::atomic::Ordering::Relaxed),
                    "MEDICO-Monitoring-Check"
                );
                let _ = result;
            }
        });
    }

    /// Liefert die aktuellen Dashboard-Daten.
    pub async fn get_monitoring(&self) -> MonitoringData {
        let requests = self.requests.lock().unwrap();
        let request_count = requests.len();
        let error_count = requests.iter().filter(|r| r.is_error).count() as u32;

        let avg_latency = if request_count > 0 {
            Some(
                requests
                    .iter()
                    .map(|r| r.latency_ms)
                    .sum::<u64>()
                    / request_count as u64,
            )
        } else {
            None
        };

        let recent: Vec<RequestStat> = requests.iter().rev().take(10).cloned().collect();

        let error_rate = if request_count > 0 {
            error_count as f64 / request_count as f64
        } else {
            0.0
        };

        let health = self.current_health();
        let consecutive_errors = self
            .consecutive_errors
            .load(std::sync::atomic::Ordering::Relaxed);

        MonitoringData {
            health,
            consecutive_errors,
            last_ping_ok: self
                .last_ping_ok
                .lock()
                .unwrap()
                .map(|t| t.to_rfc3339()),
            last_ping_fail: self
                .last_ping_fail
                .lock()
                .unwrap()
                .map(|t| t.to_rfc3339()),
            last_latency_ms: {
                let v = self
                    .last_latency_ms
                    .load(std::sync::atomic::Ordering::Relaxed);
                if v > 0 {
                    Some(v)
                } else {
                    None
                }
            },
            avg_latency_ms: avg_latency,
            request_count,
            error_count,
            error_rate,
            recent_requests: recent,
            last_check_at: self
                .last_check_at
                .lock()
                .unwrap()
                .map(|t| t.to_rfc3339()),
        }
    }

    /// Ermittelt den aktuellen Health-Status aus dem atomic.
    fn current_health(&self) -> MedicoHealth {
        match self.health.load(std::sync::atomic::Ordering::Relaxed) {
            0 => MedicoHealth::Healthy,
            1 => MedicoHealth::Degraded,
            _ => MedicoHealth::Unreachable,
        }
    }
}

// ============================================================
// Axum-Handler
// ============================================================

use axum::extract::State;
use axum::response::Json;
use kis_core::models::{ApiResponse, ResponseMeta};

/// GET /api/v1/interfaces/medico/monitoring
pub async fn monitoring_handler(
    State(monitor): State<Arc<MedicoMonitor>>,
) -> Json<ApiResponse<MonitoringData>> {
    let data = monitor.get_monitoring().await;
    Json(ApiResponse {
        data,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    })
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_monitor() -> MedicoMonitor {
        let client = Arc::new(FhirClient::new(
            "http://localhost:9999",
            "test-client",
            "test-secret",
            "http://localhost:9999/oauth2/token",
        ));
        MedicoMonitor::new(client, None)
    }

    #[test]
    fn record_request_success() {
        let monitor = make_monitor();
        monitor.record_request(150, 200);
        assert_eq!(
            monitor
                .consecutive_errors
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert_eq!(
            monitor
                .last_latency_ms
                .load(std::sync::atomic::Ordering::Relaxed),
            150
        );
    }

    #[test]
    fn record_request_error_resets_on_success() {
        let monitor = make_monitor();
        monitor.record_request(100, 500);
        monitor.record_request(100, 500);
        assert_eq!(
            monitor
                .consecutive_errors
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        // Erfolg bricht die Fehlerkette
        monitor.record_request(100, 200);
        assert_eq!(
            monitor
                .consecutive_errors
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn three_consecutive_errors_degrades() {
        let monitor = make_monitor();
        monitor.record_request(100, 500);
        monitor.record_request(100, 500);
        monitor.record_request(100, 500);
        assert_eq!(monitor.current_health(), MedicoHealth::Degraded);
    }

    #[test]
    fn request_ring_buffer_max_100() {
        let monitor = make_monitor();
        for i in 0..150 {
            monitor.record_request(100, 200);
            let _ = i;
        }
        let requests = monitor.requests.lock().unwrap();
        assert_eq!(requests.len(), 100);
        // Älteste 50 wurden verdrängt
        assert_eq!(requests.front().unwrap().latency_ms, 100);
    }

    #[test]
    fn monitoring_data_empty() {
        let monitor = make_monitor();
        let data = tokio_test::block_on(monitor.get_monitoring());
        assert_eq!(data.health, MedicoHealth::Healthy);
        assert_eq!(data.request_count, 0);
        assert_eq!(data.error_count, 0);
        assert_eq!(data.error_rate, 0.0);
        assert!(data.last_latency_ms.is_none());
        assert!(data.avg_latency_ms.is_none());
    }

    #[test]
    fn monitoring_data_with_requests() {
        let monitor = make_monitor();
        monitor.record_request(100, 200);
        monitor.record_request(200, 500);
        monitor.record_request(300, 200);
        let data = tokio_test::block_on(monitor.get_monitoring());
        assert_eq!(data.request_count, 3);
        assert_eq!(data.error_count, 1);
        assert!((data.error_rate - 1.0 / 3.0).abs() < 0.01);
        assert_eq!(data.avg_latency_ms, Some(200)); // (100+200+300)/3 = 200
        assert_eq!(data.recent_requests.len(), 3);
    }

    #[test]
    fn health_serialize_lowercase() {
        let json = serde_json::to_string(&MedicoHealth::Healthy).unwrap();
        assert_eq!(json, "\"healthy\"");
        let json = serde_json::to_string(&MedicoHealth::Degraded).unwrap();
        assert_eq!(json, "\"degraded\"");
        let json = serde_json::to_string(&MedicoHealth::Unreachable).unwrap();
        assert_eq!(json, "\"unreachable\"");
    }
}
