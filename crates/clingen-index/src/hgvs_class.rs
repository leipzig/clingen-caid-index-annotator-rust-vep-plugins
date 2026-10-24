//! The kind of edit a Registry HGVS string names, for per-class reports.
//!
//! `validate` and `reach` both break their counts down this way, so a class
//! means the same in either report.

/// Kind of edit named by a Registry HGVS string, for the per-class report.
///
/// Declaration order is the report order; the discriminants index the
/// per-class counts (`[u64; Class::ALL.len()]`).
#[derive(Clone, Copy)]
pub(crate) enum Class {
    Substitution,
    Del,
    Dup,
    Ins,
    Delins,
    Mito,
    Other,
}

impl Class {
    /// Report order.
    pub(crate) const ALL: [Class; 7] = [
        Class::Substitution,
        Class::Del,
        Class::Dup,
        Class::Ins,
        Class::Delins,
        Class::Mito,
        Class::Other,
    ];

    /// The class of the edit a Registry HGVS string names.
    pub(crate) fn of(registry_hgvs: &str) -> Class {
        if registry_hgvs.contains(":m.") {
            Class::Mito
        } else if registry_hgvs.contains('>') {
            Class::Substitution
        } else if registry_hgvs.contains("delins") {
            Class::Delins
        } else if registry_hgvs.ends_with("dup") {
            Class::Dup
        } else if registry_hgvs.ends_with("del") {
            Class::Del
        } else if registry_hgvs.contains("ins") {
            Class::Ins
        } else {
            Class::Other
        }
    }

    /// The label the per-class tables and the mismatch TSV use.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Class::Substitution => "substitution",
            Class::Del => "del",
            Class::Dup => "dup",
            Class::Ins => "ins",
            Class::Delins => "delins",
            Class::Mito => "m",
            Class::Other => "other",
        }
    }
}
