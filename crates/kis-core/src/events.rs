//! Ereignisse: die einzige Verbindung zwischen Fachmodulen.
//!
//! Ein Modul meldet, was fachlich geschehen ist (`case.discharged`), und
//! weiß nicht, wer darauf reagiert. Andere Module melden über
//! [`crate::module::Module::subscribers`] Reaktionen an. So kann jedes
//! Modul geändert, ersetzt oder abgeschaltet werden, ohne dass ein anderes
//! davon erfährt.
//!
//! [`EventBus::publish`] tut zwei Dinge in der Transaktion des Aufrufers:
//!
//! 1. Es schreibt das Ereignis in die Outbox `domain_events`. Externe
//!    Schnittstellen (HL7, FHIR, Kommunikationsserver) lesen sie über einen
//!    Cursor ([`read_events`], [`acknowledge`]).
//! 2. Es ruft die angemeldeten Reaktionen auf — jede in einem eigenen
//!    Savepoint. **Schlägt eine Reaktion fehl, wird nur sie
//!    zurückgerollt; der auslösende Vorgang läuft weiter.** Der Fehler
//!    steht danach in `event_failures` und im Log, ist für die
//!    Administration sichtbar und geht nicht still verloren.
//!
//! Nutzlast: nur Schlüssel und Codes, keine Personendaten. Wer mehr
//! braucht, liest es mit den eigenen Rechten nach.

use std::sync::{Arc, RwLock};

use serde_json::Value;
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

/// Bekannte Themen. Neue Themen hier ergänzen, damit sie auffindbar sind.
pub mod topics {
    pub const CASE_ADMITTED: &str = "case.admitted";
    pub const CASE_TRANSFERRED: &str = "case.transferred";
    pub const CASE_DISCHARGED: &str = "case.discharged";
    pub const CASE_CANCELLED: &str = "case.cancelled";
    pub const ORDER_COMPLETED: &str = "order.completed";
}

/// Ein veröffentlichtes Ereignis.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Event {
    pub id: i64,
    pub topic: String,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub payload: Value,
}

/// Reaktion eines Moduls auf Ereignisse anderer Module.
#[axum::async_trait]
pub trait EventSubscriber: Send + Sync {
    /// Eindeutiger Name für Log und Fehlertabelle, z. B. `orders.automation`
    fn name(&self) -> &'static str;
    /// Themen, auf die reagiert wird
    fn topics(&self) -> &'static [&'static str];
    /// Reaktion. Läuft in einem Savepoint der auslösenden Transaktion.
    async fn handle(&self, tx: &mut PgConnection, event: &Event) -> anyhow::Result<()>;
}

/// Verteilt Ereignisse an Outbox und angemeldete Reaktionen.
#[derive(Default)]
pub struct EventBus {
    subscribers: RwLock<Vec<Arc<dyn EventSubscriber>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Meldet eine Reaktion an (beim Zusammenbau der Module).
    pub fn register(&self, subscriber: Arc<dyn EventSubscriber>) {
        self.subscribers
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push(subscriber);
    }

    /// Namen der angemeldeten Reaktionen (Statusanzeige).
    pub fn subscriber_names(&self) -> Vec<&'static str> {
        self.subscribers
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|s| s.name())
            .collect()
    }

    /// Veröffentlicht ein Ereignis in der Transaktion des Aufrufers.
    ///
    /// Fehler beim Schreiben der Outbox brechen den Vorgang ab (das
    /// Ereignis darf nicht verloren gehen). Fehler einer Reaktion werden
    /// isoliert und protokolliert.
    pub async fn publish(
        &self,
        tx: &mut PgConnection,
        topic: &str,
        entity_type: &str,
        entity_id: Uuid,
        payload: Value,
    ) -> Result<Event, sqlx::Error> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO domain_events (topic, entity_type, entity_id, payload)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(topic)
        .bind(entity_type)
        .bind(entity_id)
        .bind(&payload)
        .fetch_one(&mut *tx)
        .await?;
        let event = Event {
            id,
            topic: topic.to_string(),
            entity_type: entity_type.to_string(),
            entity_id,
            payload,
        };

        let interested: Vec<Arc<dyn EventSubscriber>> = self
            .subscribers
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|s| s.topics().contains(&topic))
            .cloned()
            .collect();

        for subscriber in interested {
            let mut savepoint = tx.begin().await?;
            match subscriber.handle(&mut savepoint, &event).await {
                Ok(()) => savepoint.commit().await?,
                Err(error) => {
                    savepoint.rollback().await?;
                    tracing::error!(
                        subscriber = subscriber.name(),
                        topic,
                        event_id = id,
                        %error,
                        "Folgeaktion fehlgeschlagen; auslösender Vorgang läuft weiter"
                    );
                    sqlx::query(
                        "INSERT INTO event_failures (event_id, subscriber, error) VALUES ($1, $2, $3)",
                    )
                    .bind(id)
                    .bind(subscriber.name())
                    .bind(truncate(&error.to_string(), 500))
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        Ok(event)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Liest Ereignisse nach dem Cursor eines Konsumenten (ohne ihn zu
/// verschieben). `limit` wird auf 1..=500 begrenzt.
pub async fn read_events(
    pool: &PgPool,
    consumer: &str,
    limit: i64,
) -> Result<Vec<Event>, sqlx::Error> {
    let rows: Vec<(i64, String, String, Uuid, Value)> = sqlx::query_as(
        "SELECT e.id, e.topic, e.entity_type, e.entity_id, e.payload
         FROM domain_events e
         WHERE e.id > COALESCE((SELECT last_event_id FROM event_consumers WHERE name = $1), 0)
         ORDER BY e.id ASC LIMIT $2",
    )
    .bind(consumer)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, topic, entity_type, entity_id, payload)| Event {
            id,
            topic,
            entity_type,
            entity_id,
            payload,
        })
        .collect())
}

/// Verschiebt den Cursor eines Konsumenten nach vorn (nie zurück).
pub async fn acknowledge(
    pool: &PgPool,
    consumer: &str,
    up_to_event_id: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO event_consumers (name, last_event_id) VALUES ($1, $2)
         ON CONFLICT (name) DO UPDATE
         SET last_event_id = GREATEST(event_consumers.last_event_id, EXCLUDED.last_event_id),
             updated_at = now()
         RETURNING last_event_id",
    )
    .bind(consumer)
    .bind(up_to_event_id)
    .fetch_one(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("äöü", 2), "äö");
        assert_eq!(truncate("abc", 10), "abc");
    }
}
