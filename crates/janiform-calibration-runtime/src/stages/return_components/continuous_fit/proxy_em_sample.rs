use std::cmp::Ordering;

use super::budget::proxy_em_fitting_sample_target_count;
use super::stratified_representatives::weighted_representative_member_indices_from_sorted_strata;
use super::types::ReturnComponentsStageContinuousReturnCalibrationSample;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProxyEmFittingSample {
    member_indices: Vec<usize>,
    observed_returns: Vec<f64>,
    business_time_scales: Vec<f64>,
    weights: Vec<f64>,
}

impl ProxyEmFittingSample {
    pub(super) fn member_indices(&self) -> &[usize] {
        self.member_indices.as_slice()
    }

    pub(super) fn observed_returns(&self) -> &[f64] {
        self.observed_returns.as_slice()
    }

    pub(super) fn business_time_scales(&self) -> &[f64] {
        self.business_time_scales.as_slice()
    }

    pub(super) fn weights(&self) -> &[f64] {
        self.weights.as_slice()
    }

    pub(super) fn sample_count(&self) -> usize {
        self.observed_returns.len()
    }

    pub(super) fn weight_sum(&self) -> f64 {
        self.weights.iter().copied().sum()
    }
}

pub(super) fn build_proxy_em_fitting_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ProxyEmFittingSample {
    let valid_member_indices = valid_member_indices(calibration_sample);
    if valid_member_indices.is_empty() {
        return ProxyEmFittingSample {
            member_indices: Vec::new(),
            observed_returns: Vec::new(),
            business_time_scales: Vec::new(),
            weights: Vec::new(),
        };
    }

    let target_count =
        proxy_em_fitting_sample_target_count(calibration_sample).min(valid_member_indices.len());
    if target_count >= valid_member_indices.len() {
        return full_weighted_sample(calibration_sample, valid_member_indices.as_slice());
    }

    let mut representative_indices_and_weights = Vec::new();
    let strata = sign_split_scale_strata(calibration_sample, valid_member_indices.as_slice());
    for stratum in strata {
        append_stratum_representatives(
            calibration_sample,
            stratum,
            target_count,
            valid_member_indices.len(),
            &mut representative_indices_and_weights,
        );
    }

    representative_indices_and_weights.sort_by(|left, right| left.0.cmp(&right.0));

    let observed_returns = representative_indices_and_weights
        .iter()
        .map(|(member_index, _)| calibration_sample.observed_returns[*member_index])
        .collect();
    let business_time_scales = representative_indices_and_weights
        .iter()
        .map(|(member_index, _)| calibration_sample.business_time_scales[*member_index])
        .collect();
    let weights = representative_indices_and_weights
        .iter()
        .map(|(_, weight)| *weight)
        .collect();
    let member_indices = representative_indices_and_weights
        .iter()
        .map(|(member_index, _)| *member_index)
        .collect();

    ProxyEmFittingSample {
        member_indices,
        observed_returns,
        business_time_scales,
        weights,
    }
}

fn valid_member_indices(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> Vec<usize> {
    let aligned_count = calibration_sample
        .observed_returns
        .len()
        .min(calibration_sample.business_time_scales.len());

    (0..aligned_count)
        .filter(|member_index| {
            let observed_return = calibration_sample.observed_returns[*member_index];
            let business_time_scale = calibration_sample.business_time_scales[*member_index];

            observed_return.is_finite()
                && business_time_scale.is_finite()
                && business_time_scale > 0.0
        })
        .collect()
}

fn full_weighted_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    member_indices: &[usize],
) -> ProxyEmFittingSample {
    ProxyEmFittingSample {
        member_indices: member_indices.to_vec(),
        observed_returns: member_indices
            .iter()
            .map(|member_index| calibration_sample.observed_returns[*member_index])
            .collect(),
        business_time_scales: member_indices
            .iter()
            .map(|member_index| calibration_sample.business_time_scales[*member_index])
            .collect(),
        weights: vec![1.0; member_indices.len()],
    }
}

fn sign_split_scale_strata(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    valid_member_indices: &[usize],
) -> Vec<Vec<usize>> {
    let scale_strata = scale_strata(calibration_sample, valid_member_indices);
    let mut strata = Vec::new();

    for scale_stratum in scale_strata {
        let mut negative_returns = Vec::new();
        let mut zero_returns = Vec::new();
        let mut positive_returns = Vec::new();

        for member_index in scale_stratum {
            let observed_return = calibration_sample.observed_returns[member_index];
            match observed_return.total_cmp(&0.0) {
                Ordering::Less => negative_returns.push(member_index),
                Ordering::Equal => zero_returns.push(member_index),
                Ordering::Greater => positive_returns.push(member_index),
            }
        }

        push_nonempty_stratum(&mut strata, negative_returns);
        push_nonempty_stratum(&mut strata, zero_returns);
        push_nonempty_stratum(&mut strata, positive_returns);
    }

    strata
}

fn scale_strata(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    valid_member_indices: &[usize],
) -> Vec<Vec<usize>> {
    if calibration_sample.scale_bucket_profiles.is_empty() {
        return vec![valid_member_indices.to_vec()];
    }

    let mut covered_member_indices = vec![false; calibration_sample.observed_returns.len()];
    let mut strata = Vec::new();

    for scale_bucket_profile in &calibration_sample.scale_bucket_profiles {
        let mut scale_stratum = Vec::new();

        for member_index in scale_bucket_profile.member_indices.iter().copied() {
            if is_valid_member_index(calibration_sample, member_index) {
                covered_member_indices[member_index] = true;
                scale_stratum.push(member_index);
            }
        }

        push_nonempty_stratum(&mut strata, scale_stratum);
    }

    let uncovered_valid_members = valid_member_indices
        .iter()
        .copied()
        .filter(|member_index| !covered_member_indices[*member_index])
        .collect();
    push_nonempty_stratum(&mut strata, uncovered_valid_members);

    strata
}

fn is_valid_member_index(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    member_index: usize,
) -> bool {
    if member_index >= calibration_sample.observed_returns.len()
        || member_index >= calibration_sample.business_time_scales.len()
    {
        return false;
    }

    let observed_return = calibration_sample.observed_returns[member_index];
    let business_time_scale = calibration_sample.business_time_scales[member_index];

    observed_return.is_finite() && business_time_scale.is_finite() && business_time_scale > 0.0
}

fn push_nonempty_stratum(strata: &mut Vec<Vec<usize>>, stratum: Vec<usize>) {
    if !stratum.is_empty() {
        strata.push(stratum);
    }
}

fn append_stratum_representatives(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    mut stratum: Vec<usize>,
    total_target_count: usize,
    total_valid_count: usize,
    output: &mut Vec<(usize, f64)>,
) {
    if stratum.is_empty() || total_target_count == 0 || total_valid_count == 0 {
        return;
    }

    stratum.sort_by(|left_member_index, right_member_index| {
        let left_abs_return = calibration_sample.observed_returns[*left_member_index].abs();
        let right_abs_return = calibration_sample.observed_returns[*right_member_index].abs();

        left_abs_return
            .total_cmp(&right_abs_return)
            .then_with(|| {
                calibration_sample.business_time_scales[*left_member_index]
                    .total_cmp(&calibration_sample.business_time_scales[*right_member_index])
            })
            .then_with(|| left_member_index.cmp(right_member_index))
    });

    let stratum_target_count = total_target_count
        .saturating_mul(stratum.len())
        .div_ceil(total_valid_count)
        .clamp(1, stratum.len());

    output.extend(weighted_representative_member_indices_from_sorted_strata(
        stratum.as_slice(),
        stratum_target_count,
    ));
}

#[cfg(test)]
mod tests {
    use super::build_proxy_em_fitting_sample;
    use crate::stages::return_components::continuous_fit::sample::build_continuous_return_calibration_sample_from_components;

    #[test]
    fn proxy_em_fitting_sample_keeps_small_samples_exact() {
        let calibration_sample = build_continuous_return_calibration_sample_from_components(
            vec![-0.2, 0.0, 0.3],
            vec![1.0, 2.0, 3.0],
            vec![0.5, 0.5, 0.5],
            0.5,
        );

        let fitting_sample = build_proxy_em_fitting_sample(&calibration_sample);

        assert_eq!(fitting_sample.observed_returns(), [-0.2, 0.0, 0.3]);
        assert_eq!(fitting_sample.business_time_scales(), [1.0, 2.0, 3.0]);
        assert_eq!(fitting_sample.weights(), [1.0, 1.0, 1.0]);
        assert_eq!(fitting_sample.weight_sum(), 3.0);
    }

    #[test]
    fn proxy_em_fitting_sample_compresses_large_samples_with_exact_total_weight() {
        let observed_returns: Vec<f64> = (0..10_000)
            .map(|member_number| {
                let signed_rank = member_number as f64 - 5_000.0;
                signed_rank / 100_000.0
            })
            .collect();
        let business_time_scales: Vec<f64> = (0..10_000)
            .map(|member_number| 1.0 + (member_number % 7) as f64)
            .collect();
        let sell_share_proxies = vec![0.5; 10_000];
        let calibration_sample = build_continuous_return_calibration_sample_from_components(
            observed_returns,
            business_time_scales,
            sell_share_proxies,
            0.5,
        );

        let fitting_sample = build_proxy_em_fitting_sample(&calibration_sample);

        assert!(fitting_sample.sample_count() < 10_000);
        assert_eq!(fitting_sample.weight_sum(), 10_000.0);
        assert!(
            fitting_sample
                .observed_returns()
                .iter()
                .any(|observed_return| *observed_return < 0.0)
        );
        assert!(
            fitting_sample
                .observed_returns()
                .iter()
                .any(|observed_return| *observed_return > 0.0)
        );
    }
}
