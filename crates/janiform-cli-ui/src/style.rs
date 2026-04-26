use console::Style;

/// Semantic style names used by command implementations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticStyle {
    /// Dim label text.
    Label,
    /// Dim secondary metadata text.
    Metadata,
    /// Default foreground value text.
    Value,
    /// Important path or artifact reference.
    ImportantPath,
    /// Active stage or phase label.
    ActiveStage,
    /// Completed marker or status.
    CompletedMarker,
    /// Warning marker or status.
    WarningMarker,
    /// Failure marker or status.
    FailureMarker,
    /// Bold table header text.
    TableHeader,
}

pub(crate) fn style_contract(style: SemanticStyle) -> Style {
    match style {
        SemanticStyle::Label | SemanticStyle::Metadata => Style::new().dim(),
        SemanticStyle::Value => Style::new(),
        SemanticStyle::ImportantPath | SemanticStyle::ActiveStage => Style::new().cyan(),
        SemanticStyle::CompletedMarker => Style::new().green(),
        SemanticStyle::WarningMarker => Style::new().yellow(),
        SemanticStyle::FailureMarker => Style::new().red(),
        SemanticStyle::TableHeader => Style::new().bold(),
    }
}
