//! KIS-Anwendung: Router, Middleware, TLS und Shutdown.
//!
//! Als Bibliothek ausgelegt, damit Integrationstests den echten Router
//! gegen eine echte PostgreSQL-Datenbank prüfen können.

pub mod api;
pub mod graceful_shutdown;
pub mod tls;
