pub mod dicom;
pub mod fhir;
pub mod hl7;
pub mod xds;

pub mod medico {
    //! CGM MEDICO Integration (FHIR R4, OAuth 2.0, GDT)
    //!
    //! Siehe `docs/CGM_MEDICO.md` für die vollständige Dokumentation.
    pub use crate::schnittstellen::fhir::medico::*;
}
