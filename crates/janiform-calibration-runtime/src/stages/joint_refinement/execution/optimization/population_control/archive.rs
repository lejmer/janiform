use super::super::super::ordered_parameter_bits;
use super::PopulationControlCandidateRecord;

pub(super) fn best_population_control_score_record(
    archive: &[PopulationControlCandidateRecord],
) -> Option<&PopulationControlCandidateRecord> {
    archive
        .iter()
        .min_by(|left, right| left.score.compare(&right.score))
}

pub(super) fn ranked_population_control_archive_records(
    archive: &[PopulationControlCandidateRecord],
) -> Vec<&PopulationControlCandidateRecord> {
    let mut records = archive.iter().collect::<Vec<_>>();

    records.sort_by(|left, right| {
        left.score
            .compare(&right.score)
            .then_with(|| left.restart_id.cmp(&right.restart_id))
            .then_with(|| {
                left.candidate_evaluation_count
                    .cmp(&right.candidate_evaluation_count)
            })
    });

    records
}

pub(super) fn distinct_ranked_population_control_archive_record(
    archive: &[PopulationControlCandidateRecord],
    archive_position: usize,
) -> Option<&PopulationControlCandidateRecord> {
    let mut seen_parameter_bits = Vec::new();
    let mut distinct_position = 0usize;

    for record in ranked_population_control_archive_records(archive) {
        let parameter_bits = ordered_parameter_bits(record.parameter_vector.as_slice());
        if seen_parameter_bits
            .iter()
            .any(|seen_bits: &Vec<u64>| seen_bits == &parameter_bits)
        {
            continue;
        }

        if distinct_position == archive_position {
            return Some(record);
        }

        seen_parameter_bits.push(parameter_bits);

        distinct_position = distinct_position.saturating_add(1);
    }

    None
}

pub(super) fn best_restart_population_control_archive_record<'a>(
    archive: &'a [PopulationControlCandidateRecord],
    restart_id: &str,
) -> Option<&'a PopulationControlCandidateRecord> {
    archive
        .iter()
        .filter(|record| record.restart_id == restart_id)
        .min_by(|left, right| left.score.compare(&right.score))
}
