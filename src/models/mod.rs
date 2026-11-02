use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patient {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub birth_date: Option<String>,
    pub insurance_number: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appointment {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub doctor_id: Option<Uuid>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub status: AppointmentStatus,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AppointmentStatus {
    Scheduled,
    Confirmed,
    Completed,
    Cancelled,
    NoShow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doctor {
    pub id: Uuid,
    pub name: String,
    pub specialty: String,
    pub license_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medication {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub name: String,
    pub dosage: String,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub prescriber_id: Option<Uuid>,
}
