//! Session-Management (Task 5.5)
//!
//! In-Memory-Session-Store für aktive Login-Sessions:
//! - Concurrent-Login-Limit: max. 5 Sessions pro User (konfigurierbar)
//! - Auto-Logout: 30 min Inaktivität → Session ungültig
//! - `check_and_refresh(session_id)`: verlängert `last_activity`, prüft Inaktivität
//! - `revoke_all_sessions(user_id)`: alle Sessions eines Users löschen
//! - `active_sessions(user_id)`: Liste der aktiven Sessions
//!
//! Architektur: `std::sync::RwLock<HashMap>` (keine zusätzliche Abhängigkeit,
//! Sperren werden nur für den HashMap-Zugriff gehalten, nie über `.await`).
//! Sessions sind prozesslokal — bei mehreren Instanzen muss eine zentrale
//! Store-Implementierung (z.B. Redis) hinter derselben öffentlichen API treten.
//!
//! Testbarkeit: Die Uhr ist injizierbar ([`SessionStore::with_clock`]), damit
//! Inaktivitäts-Timeouts deterministisch getestet werden können.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::security::Role;

/// Standard-Limit für gleichzeitige Sessions pro User.
pub const DEFAULT_CONCURRENT_LIMIT: u32 = 5;

/// Standard-Inaktivitäts-Timeout (30 Minuten) für den Auto-Logout.
pub const DEFAULT_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Eine aktive Login-Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Session-ID (UUID)
    pub session_id: String,
    /// User, der die Session besitzt
    pub user_id: String,
    /// Rolle des Users zum Login-Zeitpunkt
    pub role: Role,
    /// Zeitpunkt des Logins
    pub created_at: DateTime<Utc>,
    /// Zeitpunkt der letzten Aktivität (wird bei jedem Request aktualisiert)
    pub last_activity: DateTime<Utc>,
    /// Concurrent-Limit, das für diesen User gilt
    pub concurrent_limit: u32,
}

impl Session {
    /// Erzeugt eine neue Session mit `created_at = last_activity = now`.
    pub fn new(user_id: &str, role: Role, concurrent_limit: u32, now: DateTime<Utc>) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            role,
            created_at: now,
            last_activity: now,
            concurrent_limit,
        }
    }
}

/// Ergebnis eines Inaktivitäts-Checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session ist aktiv (Inaktivitäts-Timeout nicht überschritten).
    Active,
    /// Session wurde durch Inaktivität automatisch beendet (Auto-Logout).
    Inactive,
    /// Session existiert nicht (ungültige ID oder bereits widerrufen).
    Unknown,
}

/// Abstrakte Uhr — ermöglicht deterministische Unit-Tests.
pub trait Clock: Send + Sync {
    /// Aktueller Zeitpunkt (Wall-Clock, UTC)
    fn now(&self) -> DateTime<Utc>;
}

/// Produktions-Uhr: [`Utc::now`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// In-Memory-Store für aktive Sessions.
///
/// Alle Methoden sind `Send + Sync` und können hinter `Arc` in Axum-State
/// gehalten werden. Zeitbasierte Checks (Auto-Logout) werden bei jedem
/// Zugriff lazy ausgeführt.
#[derive(Debug)]
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Session>>,
    /// Max. gleichzeitige Sessions pro User (Default 5, konfigurierbar)
    concurrent_limit: u32,
    /// Inaktivitäts-Timeout (Default 30 min)
    inactivity_timeout: Duration,
    /// Zeitquelle (Default: [`SystemClock`], in Tests injizierbar)
    clock: Arc<dyn Clock>,
}

impl SessionStore {
    /// Erzeugt einen Store mit Default-Limits (5 Sessions, 30 min Inaktivität).
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            concurrent_limit: DEFAULT_CONCURRENT_LIMIT,
            inactivity_timeout: DEFAULT_INACTIVITY_TIMEOUT,
            clock: Arc::new(SystemClock),
        }
    }

    /// Erzeugt einen Store mit konfigurierbaren Limits.
    pub fn with_limits(concurrent_limit: u32, inactivity_timeout: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            concurrent_limit,
            inactivity_timeout,
            clock: Arc::new(SystemClock),
        }
    }

    /// Erzeugt einen Store mit injizierter Uhr (für Tests).
    #[doc(hidden)]
    pub fn with_limits_and_clock(
        concurrent_limit: u32,
        inactivity_timeout: Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            concurrent_limit,
            inactivity_timeout,
            clock,
        }
    }

    /// Registriert eine neue Session nach dem Login.
    ///
    /// Prüft das Concurrent-Login-Limit: wenn der User bereits
    /// `concurrent_limit` aktive Sessions hat, wird die älteste Session
    /// verdrängt (First-In-First-Out) — der User darf sich trotzdem
    /// erneut einloggen.
    ///
    /// Rückgabe: die neue Session.
    pub fn create_session(&self, user_id: &str, role: Role) -> Session {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let limit = self.concurrent_limit;
        let now = self.clock.now();

        // Inaktive Sessions vor der Zählung aufräumen
        self.evict_inactive_locked(&mut guard, now);

        // Concurrent-Limit: älteste Session verdrängen, wenn voll
        let mut user_sessions: Vec<Session> = guard
            .values()
            .filter(|s| s.user_id == user_id)
            .cloned()
            .collect();
        user_sessions.sort_by_key(|s| s.created_at);
        while user_sessions.len() >= limit as usize {
            let oldest = user_sessions.remove(0);
            guard.remove(&oldest.session_id);
        }

        let session = Session::new(user_id, role, limit, now);
        guard.insert(session.session_id.clone(), session.clone());
        session
    }

    /// Aktualisiert `last_activity` und prüft die Inaktivität.
    ///
    /// - Session existiert und ist aktiv → `Active` (Activity wird aktualisiert)
    /// - Session existiert, aber 30+ min inaktiv → `Inactive` (Session wird gelöscht)
    /// - Session existiert nicht → `Unknown`
    pub fn check_and_refresh(&self, session_id: &str) -> SessionState {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();

        match guard.get_mut(session_id) {
            Some(session) => {
                let idle = now.signed_duration_since(session.last_activity);
                if idle > chrono::Duration::from_std(self.inactivity_timeout).unwrap() {
                    // Auto-Logout: Session ungültig
                    let user_id = session.user_id.clone();
                    let idle_secs = idle.num_seconds();
                    drop(session); // Borrow beenden, bevor wir remove aufrufen
                    guard.remove(session_id);
                    tracing::info!(
                        session_id,
                        user_id = %user_id,
                        idle_secs,
                        "Session durch Inaktivität beendet (Auto-Logout)"
                    );
                    SessionState::Inactive
                } else {
                    session.last_activity = now;
                    SessionState::Active
                }
            }
            None => SessionState::Unknown,
        }
    }

    /// Beendet eine einzelne Session (Logout).
    ///
    /// Rückgabe: `true`, wenn die Session existierte und beendet wurde.
    pub fn revoke_session(&self, session_id: &str) -> bool {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let removed = guard.remove(session_id).is_some();
        if removed {
            tracing::info!(session_id, "Session beendet (Logout)");
        }
        removed
    }

    /// Beendet ALLE Sessions eines Users (z.B. bei Passwort-Reset
    /// oder "alle Geräte abmelden").
    ///
    /// Rückgabe: Anzahl der gelöschten Sessions.
    pub fn revoke_all_sessions(&self, user_id: &str) -> usize {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        self.evict_inactive_locked(&mut guard, now);

        let ids: Vec<String> = guard
            .values()
            .filter(|s| s.user_id == user_id)
            .map(|s| s.session_id.clone())
            .collect();
        for id in &ids {
            guard.remove(id);
        }
        if !ids.is_empty() {
            tracing::info!(user_id, count = ids.len(), "Alle Sessions widerrufen");
        }
        ids.len()
    }

    /// Listet die aktiven Sessions eines Users (älteste zuerst).
    ///
    /// Inaktive Sessions werden dabei lazy aufgeräumt.
    pub fn active_sessions(&self, user_id: &str) -> Vec<Session> {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        self.evict_inactive_locked(&mut guard, now);

        let mut sessions: Vec<Session> = guard
            .values()
            .filter(|s| s.user_id == user_id)
            .cloned()
            .collect();
        sessions.sort_by_key(|s| s.created_at);
        sessions
    }

    /// Anzahl aller aktiven Sessions (alle User).
    pub fn total_active(&self) -> usize {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let now = self.clock.now();
        self.evict_inactive_locked(&mut guard, now);
        guard.len()
    }

    /// Konfiguriertes Concurrent-Limit pro User.
    pub fn concurrent_limit(&self) -> u32 {
        self.concurrent_limit
    }

    /// Konfigurierter Inaktivitäts-Timeout.
    pub fn inactivity_timeout(&self) -> Duration {
        self.inactivity_timeout
    }

    // ------------------------------------------------------------------
    // Interne Helfer
    // ------------------------------------------------------------------

    /// Entfernt alle inaktiven Sessions (lazy Cleanup).
    ///
    /// MUSS mit geschriebenem Lock aufgerufen werden.
    fn evict_inactive_locked(&self, guard: &mut HashMap<String, Session>, now: DateTime<Utc>) {
        guard.retain(|_id, session| {
            let idle = now.signed_duration_since(session.last_activity);
            idle <= chrono::Duration::from_std(self.inactivity_timeout).unwrap()
        });
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Unit-Tests (Task 5.5)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-Uhr: Zeit verläuft nur, wenn `advance_secs` aufgerufen wird.
    #[derive(Debug, Default)]
    struct FakeClock {
        t: RwLock<DateTime<Utc>>,
    }

    impl FakeClock {
        fn set(&self, t: DateTime<Utc>) {
            *self.t.write().unwrap() = t;
        }

        fn advance_secs(&self, secs: i64) {
            let mut guard = self.t.write().unwrap();
            *guard += chrono::Duration::seconds(secs);
        }

        fn now(&self) -> DateTime<Utc> {
            *self.t.read().unwrap()
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> DateTime<Utc> {
            *self.t.read().unwrap()
        }
    }

    fn t0() -> DateTime<Utc> {
        // Fester Referenzzeitpunkt (2026-01-01 00:00 UTC)
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
    }

    /// Helper: Store mit 5-Sessions-Limit, 30-min-Timeout, FakeClock bei t0.
    fn test_store(clock: Arc<FakeClock>) -> SessionStore {
        clock.set(t0());
        SessionStore::with_limits_and_clock(
            DEFAULT_CONCURRENT_LIMIT,
            DEFAULT_INACTIVITY_TIMEOUT,
            clock as Arc<dyn Clock>,
        )
    }

    /// Inaktivitäts-Timeout: Session bleibt aktiv nach 29 min,
    /// wird nach 31 min automatisch beendet.
    #[test]
    fn test_inactivity_timeout() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock.clone());

        let session = store.create_session("user-1", Role::Admin);

        // Nach 29 min: noch aktiv
        clock.advance_secs(29 * 60);
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Active);

        // Weitere 2 min (31 min insgesamt): Auto-Logout
        clock.advance_secs(2 * 60);
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Inactive);

        // Session ist danach weg
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Unknown);
        assert!(store.active_sessions("user-1").is_empty());
    }

    /// check_and_refresh verlängert last_activity: erneute Aktivität
    /// innerhalb des Fensters hält die Session am Leben.
    #[test]
    fn test_refresh_extends_activity() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock.clone());

        let session = store.create_session("user-1", Role::Arzt);

        // 25 min inaktiv, dann Aktivität → Session lebt weiter
        clock.advance_secs(25 * 60);
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Active);

        // Weitere 25 min (50 min seit Login, aber nur 25 seit Aktivität) → aktiv
        clock.advance_secs(25 * 60);
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Active);

        // Weitere 10 min (35 min seit letzter Aktivität) → Auto-Logout
        clock.advance_secs(10 * 60);
        assert_eq!(store.check_and_refresh(&session.session_id), SessionState::Inactive);
    }

    /// Concurrent-Limit: max. 5 Sessions pro User; die 6. verdrängt die älteste.
    #[test]
    fn test_concurrent_limit() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock.clone());

        let user = "user-limit";
        for _ in 0..5 {
            store.create_session(user, Role::Arzt);
        }
        assert_eq!(store.active_sessions(user).len(), 5, "Limit erreicht");

        // Älteste Session merken
        let oldest_id = store.active_sessions(user)[0].session_id.clone();

        // 6. Session: die älteste wird verdrängt
        store.create_session(user, Role::Arzt);
        let sessions = store.active_sessions(user);
        assert_eq!(sessions.len(), 5, "Verdrängung hält Limit bei 5");
        assert!(
            !sessions.iter().any(|s| s.session_id == oldest_id),
            "älteste Session wurde verdrängt"
        );
        assert!(
            store
                .active_sessions("other-user")
                .is_empty(),
            "andere User nicht betroffen"
        );
    }

    /// Revoke: Einzelne Session beenden.
    #[test]
    fn test_revoke_session() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock);

        let s1 = store.create_session("user-1", Role::Pflege);
        let s2 = store.create_session("user-1", Role::Pflege);

        assert!(store.revoke_session(&s1.session_id));
        assert_eq!(store.active_sessions("user-1").len(), 1);

        // Doppelte Beendigung: Session ist schon weg
        assert!(!store.revoke_session(&s1.session_id));
        assert!(store.revoke_session(&s2.session_id));
        assert!(store.active_sessions("user-1").is_empty());
    }

    /// Revoke-All: alle Sessions eines Users löschen, andere User bleiben.
    #[test]
    fn test_revoke_all_sessions() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock);

        for _ in 0..3 {
            store.create_session("user-a", Role::Arzt);
        }
        store.create_session("user-b", Role::Pflege);
        store.create_session("user-b", Role::Pflege);

        assert_eq!(store.total_active(), 5);
        let revoked = store.revoke_all_sessions("user-a");
        assert_eq!(revoked, 3);
        assert!(store.active_sessions("user-a").is_empty());
        assert_eq!(store.active_sessions("user-b").len(), 2, "andere User bleiben");
    }

    /// Revoke-All ohne Sessions: liefert 0, kein Panic.
    #[test]
    fn test_revoke_all_sessions_empty() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock);
        assert_eq!(store.revoke_all_sessions("nobody"), 0);
    }

    /// Active-Sessions: vollständige Liste inkl. aller Felder, sortiert.
    #[test]
    fn test_active_sessions() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock);

        store.create_session("user-1", Role::Rezeption);
        store.create_session("user-1", Role::Rezeption);

        let sessions = store.active_sessions("user-1");
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|s| s.user_id == "user-1"));
        assert!(sessions.iter().all(|s| s.role == Role::Rezeption));
        assert!(sessions.iter().all(|s| s.concurrent_limit == 5));
        // Sortiert nach created_at (älteste zuerst)
        assert!(sessions[0].created_at <= sessions[1].created_at);
        // IDs sind eindeutig
        assert_ne!(sessions[0].session_id, sessions[1].session_id);
    }

    /// Konfigurierbares Limit: Store mit Limit 2 verdrängt ab der 3. Session.
    #[test]
    fn test_configurable_limit() {
        let clock = Arc::new(FakeClock::default());
        clock.set(t0());
        let store = SessionStore::with_limits_and_clock(
            2,
            DEFAULT_INACTIVITY_TIMEOUT,
            clock as Arc<dyn Clock>,
        );

        store.create_session("user-1", Role::Viewer);
        store.create_session("user-1", Role::Viewer);
        assert_eq!(store.active_sessions("user-1").len(), 2);

        // 3. Login verdrängt die älteste
        store.create_session("user-1", Role::Viewer);
        assert_eq!(store.active_sessions("user-1").len(), 2);
        assert_eq!(store.concurrent_limit(), 2);
        assert_eq!(store.inactivity_timeout(), DEFAULT_INACTIVITY_TIMEOUT);
    }

    /// Check auf unbekannte Session-ID → Unknown.
    #[test]
    fn test_unknown_session() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock);
        assert_eq!(store.check_and_refresh("does-not-exist"), SessionState::Unknown);
    }

    /// Lazy-Cleanup: inaktive Sessions verschwinden bei jedem Store-Zugriff.
    #[test]
    fn test_lazy_cleanup() {
        let clock = Arc::new(FakeClock::default());
        let store = test_store(clock.clone());

        let s1 = store.create_session("user-1", Role::Viewer);
        store.create_session("user-2", Role::Viewer);
        assert_eq!(store.total_active(), 2);

        // Beide inaktiv (31 min)
        clock.advance_secs(31 * 60);
        // total_active räumt lazy auf
        assert_eq!(store.total_active(), 0);
        let _ = s1;
    }
}
