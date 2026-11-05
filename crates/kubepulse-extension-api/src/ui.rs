//! Declarative interfaces. Extensions describe what to show, and the host
//! renders it with its own components. Log, terminal, and editor surfaces are
//! opened by reference, so their high-rate data never crosses the contract.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FormFieldKind {
    Text { value: String },
    Toggle { value: bool },
    Choice { options: Vec<String>, value: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub kind: FormFieldKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub title: String,
    pub series: Vec<ChartSeries>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceKind {
    Log,
    Terminal,
    Editor,
}

/// A host-owned surface, opened with [`HostOperation::OpenSurface`](crate::HostOperation::OpenSurface).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceRef(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    pub kind: SurfaceKind,
    pub reference: SurfaceRef,
}

/// One element of a declarative interface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "element", rename_all = "kebab-case")]
pub enum UiNode {
    Text {
        text: String,
    },
    Stack {
        direction: Direction,
        children: Vec<UiNode>,
    },
    Table {
        table: Table,
    },
    DescriptionList {
        items: Vec<Description>,
    },
    /// Submitting sends the field values back as
    /// [`ExtensionRequest::SubmitForm`](crate::ExtensionRequest::SubmitForm).
    Form {
        id: String,
        fields: Vec<FormField>,
        submit_label: String,
    },
    Chart {
        chart: Chart,
    },
    Surface {
        surface: Surface,
    },
    /// An element this host does not know, for example from a newer contract.
    /// The host shows a placeholder in its place.
    #[serde(other)]
    Unknown,
}
