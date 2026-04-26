use janiform_cli_ui::{
    ArtifactPathGroup, CompactTable, CompletionSummary, CompletionSummaryStatus, FailureBlock,
    KeyValueRow, SemanticStyle, StageChecklist, StageChecklistItem, StageChecklistStatus,
    StartSummary, TerminalRenderer, WarningBlock, format_count, format_duration_seconds,
};

#[test]
fn summary_renderer_produces_scan_friendly_monochrome_output() {
    let renderer = TerminalRenderer::new(false, Some(72));
    let summary = StartSummary {
        title: "run".to_string(),
        rows: vec![
            KeyValueRow::new("run id", "fit-20260426"),
            KeyValueRow::styled(
                "output path",
                ".janiform/runs/fit",
                SemanticStyle::ImportantPath,
            ),
        ],
    };

    assert_eq!(
        summary.render(&renderer),
        "run\nrun id     : fit-20260426\noutput path: .janiform/runs/fit"
    );
}

#[test]
fn renderer_applies_ansi_only_when_color_is_enabled() {
    let monochrome = TerminalRenderer::new(false, None);
    let color = TerminalRenderer::new(true, None);
    let warning = WarningBlock {
        title: "warnings".to_string(),
        warnings: vec!["calendar gap detected".to_string()],
    };

    assert!(!warning.render(&monochrome).contains("\u{1b}["));
    assert!(warning.render(&color).contains("\u{1b}["));
}

#[test]
fn failure_and_completion_blocks_do_not_depend_on_color_for_meaning() {
    let renderer = TerminalRenderer::new(false, None);
    let failure = FailureBlock {
        title: "failed".to_string(),
        reason: "invalid run contract".to_string(),
        details: vec![KeyValueRow::new("field", "execution_input_path")],
    };
    let completion = CompletionSummary {
        title: "complete".to_string(),
        status: CompletionSummaryStatus::Succeeded,
        rows: vec![KeyValueRow::new("warnings", "0 warnings")],
    };

    assert_eq!(
        failure.render(&renderer),
        "✗ failed\n\nReason: invalid run contract\nfield : execution_input_path"
    );
    assert_eq!(
        completion.render(&renderer),
        "✓ complete\n\nStatus  : succeeded\nwarnings: 0 warnings"
    );
}

#[test]
fn compact_table_uses_plain_table_layout_with_narrow_wrapping() {
    let renderer = TerminalRenderer::new(false, Some(12));
    let table = CompactTable {
        headers: vec!["stage".to_string(), "status".to_string()],
        rows: vec![
            vec!["seasonality".to_string(), "done".to_string()],
            vec!["hawkes".to_string(), "active".to_string()],
        ],
    };

    assert_eq!(
        table.render(&renderer),
        "stag  stat\ne     us\nseas  done\nonal\nity\nhawk  acti\nes    ve"
    );
}

#[test]
fn stage_checklist_and_artifact_groups_are_plain_text_meaningful() {
    let renderer = TerminalRenderer::new(false, None);
    let checklist = StageChecklist {
        title: "stages".to_string(),
        stages: vec![
            StageChecklistItem {
                name: "seasonality".to_string(),
                status: StageChecklistStatus::Completed,
            },
            StageChecklistItem {
                name: "hawkes".to_string(),
                status: StageChecklistStatus::Active,
            },
        ],
    };
    let artifacts = ArtifactPathGroup {
        title: "artifacts".to_string(),
        paths: vec![".janiform/runs/fit/model.json".to_string()],
    };

    assert_eq!(
        checklist.render(&renderer),
        "stages\n✓ seasonality\n-> hawkes"
    );
    assert_eq!(
        artifacts.render(&renderer),
        "artifacts\n.janiform/runs/fit/model.json"
    );
}

#[test]
fn formatting_helpers_keep_units_explicit() {
    assert_eq!(format_count(1, "artifact", "artifacts"), "1 artifact");
    assert_eq!(format_count(2, "artifact", "artifacts"), "2 artifacts");
    assert_eq!(
        format_duration_seconds(std::time::Duration::from_millis(1500)),
        "1.500s"
    );
}
